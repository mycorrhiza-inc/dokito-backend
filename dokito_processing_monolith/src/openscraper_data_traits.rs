use std::convert::Infallible;

use chrono::{NaiveDate, Utc};
use dokito_types::processed::ProcessedGenericHuman;
use dokito_types::raw::RawArtificalPersonType;
use futures::future::join_all;
use futures::{FutureExt, join};
use futures_util::{StreamExt, stream};
use non_empty_string::NonEmptyString;
use thiserror::Error;
use tokio::sync::Semaphore;
use tracing::warn;
use uuid::Uuid;

use crate::data_processing_traits::{ProcessFrom, Revalidate, RevalidationOutcome};
use crate::indexes::attachment_url_index::lookup_hash_from_url;
use crate::jurisdiction_schema_mapping::FixedJurisdiction;
use crate::processing::llm_prompts::{
    clean_up_organization_name_list, split_and_fix_organization_names_blob,
};
use crate::processing::match_raw_processed::{
    match_raw_attaches_to_processed_attaches, match_raw_fillings_to_processed_fillings,
};
use crate::sql_ingester_tasks::database_author_association::{
    associate_individual_author_with_name, associate_organization_with_name,
};
use crate::sql_ingester_tasks::dokito_sql_connection::get_dokito_pool;
use crate::types::processed::{
    ProcessedGenericAttachment, ProcessedGenericDocket, ProcessedGenericFiling,
};
use crate::types::raw::{RawGenericAttachment, RawGenericDocket, RawGenericFiling};

impl Revalidate for ProcessedGenericDocket {
    async fn revalidate(&mut self) -> RevalidationOutcome {
        let mut did_change = RevalidationOutcome::NoChanges;
        if self.object_uuid.is_nil() {
            self.object_uuid = Uuid::new_v4();
            did_change = RevalidationOutcome::DidChange
        }
        if self.case_subtype.is_empty() {
            let x = self.case_type.split("-").collect::<Vec<_>>();
            if x.len() == 2 {
                let case_type = x[0].trim().to_string();
                let case_subtype = x[1].trim().to_string();
                self.case_type = case_type;
                self.case_subtype = case_subtype;
                did_change = RevalidationOutcome::DidChange;
            }
        };
        for filling in self.filings.iter_mut() {
            let did_filling_change = filling.revalidate().await;
            did_change = did_change.or(&did_filling_change);
        }
        did_change
    }
}

impl Revalidate for ProcessedGenericFiling {
    async fn revalidate(&mut self) -> RevalidationOutcome {
        let mut did_change = RevalidationOutcome::NoChanges;
        // Chance of this happening is around 1 in 3 quadrillion
        if self.object_uuid.is_nil() {
            self.object_uuid = Uuid::new_v4();
            did_change = RevalidationOutcome::DidChange
        }
        // Name stuff
        if self.name.is_empty() {
            for attach in self.attachments.iter() {
                if !attach.name.is_empty() {
                    self.name = attach.name.clone();
                    did_change = RevalidationOutcome::DidChange;
                    break;
                }
            }
        }

        for attachment in self.attachments.iter_mut() {
            let did_attachment_change = attachment.revalidate().await;
            did_change = did_change.or(&did_attachment_change);
        }
        did_change
    }
}

impl Revalidate for ProcessedGenericAttachment {
    async fn revalidate(&mut self) -> RevalidationOutcome {
        let mut did_change = RevalidationOutcome::NoChanges;
        if self.object_uuid.is_nil() {
            self.object_uuid = Uuid::new_v4();
            did_change = RevalidationOutcome::DidChange
        }
        if self.hash.is_none() && !self.url.is_empty() {
            let url = &*self.url;
            let opt_raw_attach = lookup_hash_from_url(url).await;
            if let Some(raw_attach) = opt_raw_attach {
                self.hash = Some(raw_attach.hash);
            }
        }
        did_change
    }
}
impl ProcessFrom<RawGenericDocket> for ProcessedGenericDocket {
    type ParseError = Infallible;
    type ExtraData = FixedJurisdiction;
    async fn process_from(
        input: RawGenericDocket,
        cached: Option<Self>,
        fixed_jurisdiction: Self::ExtraData,
    ) -> Result<Self, Self::ParseError> {
        let object_uuid = cached
            .as_ref()
            .map(|v| v.object_uuid)
            .unwrap_or_else(Uuid::new_v4);
        let opened_date_from_fillings = {
            let original_date = input.opened_date;
            let mut min_date = original_date;
            for filling_date in input
                .filings
                .iter()
                .filter_map(|filling| filling.filed_date)
            {
                if let Some(real_min_date) = min_date
                    && filling_date < real_min_date
                {
                    min_date = Some(filling_date);
                    if let Some(real_original_date) = original_date {
                        warn!(docket_opened_date =%real_original_date, oldest_date_found=%filling_date,"Found filling with date older then the docket opened date");
                    };
                };
            }
            // This should almost never happen, because the chances of corruption happening on the
            // docket date, and all the filling dates are very small.
            min_date.unwrap_or(NaiveDate::MAX)
        };
        let cached_fillings = cached.map(|d| d.filings);
        let matched_fillings =
            match_raw_fillings_to_processed_fillings(input.filings, cached_fillings);
        let processed_fillings_futures =
            matched_fillings
                .into_iter()
                .enumerate()
                .map(async |(index, (f_raw, f_cached))| {
                    let filling_index_data = IndexExtraData {
                        index: index as u64,
                        jurisdiction: fixed_jurisdiction,
                    };
                    let res =
                        ProcessedGenericFiling::process_from(f_raw, f_cached, filling_index_data)
                            .await;
                    let Ok(val) = res;
                    val
                });
        // Everything gets processed at once since the limiting factor on fillings is global. This
        // is to make it so that it doesnt overwhelm the system trying to process 5 dockets with
        // 10,000 fillings, but it can process 60 dockets at the same time with one filling each.
        let mut processed_fillings = join_all(processed_fillings_futures).await;

        let parties = input.case_parties;
        let mut case_parties = vec![];
        for rawparty in parties {
            if rawparty.artifical_person_type != RawArtificalPersonType::Human
                && let Ok(human_name) = NonEmptyString::try_from(rawparty.name)
            {
                let docket_govid = input.case_govid.as_str();
                tracing::error!(
                    docket_govid,
                    "Support for adding non human case parties is not supported right now, go ahead and maybe try inputing it as a petitioner."
                );
                let processed_party_huamn = ProcessedGenericHuman {
                    object_uuid: Uuid::nil(),
                    human_name,
                    western_first_name: rawparty.western_human_first_name,
                    western_last_name: rawparty.western_human_last_name,
                    contact_emails: vec![rawparty.contact_email],
                    contact_phone_numbers: vec![rawparty.contact_phone],
                    contact_addresses: vec![rawparty.contact_address],
                    representing_company: None,
                    employed_by: None,
                    title: rawparty.human_title,
                };
                case_parties.push(processed_party_huamn)
            }
        }
        processed_fillings.sort_by_key(|v| v.index_in_docket);
        let llmed_petitioner_list = split_and_fix_organization_names_blob(&input.petitioner).await;
        let final_processed_docket = ProcessedGenericDocket {
            object_uuid,
            case_parties,
            processed_at: Utc::now(),
            case_govid: input.case_govid.clone(),
            filings: processed_fillings,
            opened_date: opened_date_from_fillings,
            case_name: input.case_name.clone(),
            case_url: input.case_url.clone(),
            industry: input.industry.clone(),
            case_type: input.case_type.clone(),
            case_subtype: input.case_subtype.clone(),
            indexed_at: input.indexed_at,
            closed_date: input.closed_date,
            description: input.description.clone(),
            // Super hacky workaround until I can change the input type.
            extra_metadata: input.extra_metadata.clone().into_iter().collect(),
            hearing_officer: input.hearing_officer.clone(),
            petitioner_list: llmed_petitioner_list,
        };
        Ok(final_processed_docket)
    }
}

#[derive(Error, Debug)]
enum ProcessError {
    #[error("Encountered error syncing data with postgres.")]
    PostgresError,
}

// TODO: Might be a good idea to have a semaphore for each
static GLOBAL_SIMULTANEOUS_FILE_PROCESSING: Semaphore = Semaphore::const_new(50);

impl ProcessFrom<RawGenericFiling> for ProcessedGenericFiling {
    type ParseError = Infallible;
    type ExtraData = IndexExtraData;
    async fn process_from(
        input: RawGenericFiling,
        cached: Option<Self>,
        index_data: Self::ExtraData,
    ) -> Result<Self, Self::ParseError> {
        let _permit = GLOBAL_SIMULTANEOUS_FILE_PROCESSING.acquire().await.unwrap();
        let object_uuid = cached
            .as_ref()
            .map(|v| v.object_uuid)
            .unwrap_or_else(Uuid::new_v4);
        let _pg_pool = get_dokito_pool().unwrap();
        let (processed_attach_map, cached_orgauthorlist, cached_individualauthorllist) =
            match cached {
                Some(filling) => (
                    Some(filling.attachments),
                    Some(filling.organization_authors),
                    Some(filling.individual_authors),
                ),
                None => (None, None, None),
            };

        let matched_attach_list =
            match_raw_attaches_to_processed_attaches(input.attachments, processed_attach_map);
        // Async match the raw attachments with the cached versions, and process them async 5 at a
        // time.
        let mut processed_attachments = stream::iter(matched_attach_list.into_iter())
            .enumerate()
            .map(|(attach_index, (raw_attach, cached_attach))| {
                let attach_index_data = IndexExtraData {
                    index: attach_index as u64,
                    jurisdiction: index_data.jurisdiction,
                };
                async {
                    let res = ProcessedGenericAttachment::process_from(
                        raw_attach,
                        cached_attach,
                        attach_index_data,
                    )
                    .await;
                    let Ok(val) = res;
                    val
                }
            })
            .buffer_unordered(5)
            .collect::<Vec<_>>()
            .await;
        processed_attachments.sort_by_key(|att| att.index_in_filling);
        // Process org and individual author names.
        let mut organization_authors = {
            if let Some(org_authors) = cached_orgauthorlist {
                org_authors
            } else if input.organization_authors.is_empty() {
                split_and_fix_organization_names_blob(&input.organization_authors_blob).await
            } else {
                clean_up_organization_name_list(input.organization_authors)
            }
        };

        let mut individual_authors = {
            if let Some(individual_authors) = cached_individualauthorllist {
                individual_authors
            } else if input.individual_authors.is_empty() {
                vec![]
            } else {
                tracing::error!("Individual author logic not implemented due to corrupted data.");
                vec![]
            }
        };
        let fixed_jur = index_data.jurisdiction;
        let pool = get_dokito_pool().unwrap();

        let org_futures = organization_authors
            .iter_mut()
            .map(|org| associate_organization_with_name(org, fixed_jur, pool));
        let human_futures = individual_authors
            .iter_mut()
            .map(|human| associate_individual_author_with_name(human, fixed_jur, pool));
        let _res = join!(join_all(org_futures), join_all(human_futures));

        let proc_filling = Self {
            object_uuid,
            filed_date: input.filed_date,
            index_in_docket: index_data.index,
            attachments: processed_attachments,
            name: input.name.clone(),
            filling_govid: input.filling_govid.clone(),
            filling_url: input.filling_url.clone(),
            filing_type: input.filing_type.clone(),
            description: input.description.clone(),
            // Super hacky workaround until I can change the input type.
            extra_metadata: input.extra_metadata.clone().into_iter().collect(),
            organization_authors,
            individual_authors,
        };
        Ok(proc_filling)
    }
}

pub struct IndexExtraData {
    index: u64,
    jurisdiction: FixedJurisdiction,
}
impl ProcessFrom<RawGenericAttachment> for ProcessedGenericAttachment {
    type ParseError = Infallible;
    type ExtraData = IndexExtraData;
    async fn process_from(
        input: RawGenericAttachment,
        cached: Option<Self>,
        index_data: Self::ExtraData,
    ) -> Result<Self, Self::ParseError> {
        let uuid = cached
            .as_ref()
            .map(|val| val.object_uuid)
            .unwrap_or_else(Uuid::new_v4);
        let hash = (input.hash).or_else(|| cached.and_then(|v| v.hash));
        let return_res = Self {
            object_uuid: uuid,
            index_in_filling: index_data.index,
            name: input.name.clone(),
            document_extension: input.document_extension.clone(),
            attachment_govid: input.attachment_govid.clone(),
            attachment_type: input.attachment_type.clone(),
            attachment_subtype: input.attachment_subtype.clone(),
            url: input.url.clone(),
            extra_metadata: input.extra_metadata.clone().into_iter().collect(),
            hash,
        };
        Ok(return_res)
    }
}
