use anyhow::bail;
use dokito_types::processed::{
    ProcessedGenericDocket, ProcessedGenericFiling, ProcessedGenericHuman,
    ProcessedGenericOrganization,
};
use sqlx::{PgPool, query};
use std::collections::BTreeSet;
use uuid::Uuid;

async fn associate_individual_author_with_name(
    individual: &mut ProcessedGenericHuman,
    pool: &PgPool,
) -> Result<(), anyhow::Error> {
    if !individual.object_uuid.is_nil() {
        let author_id = individual.object_uuid;
        let result = query!("SELECT * FROM public.humans WHERE uuid=$1", author_id)
            .fetch_optional(pool)
            .await?;
        if let Some(matched_record) = result
            && matched_record.western_first_name == individual.western_first_name
            && matched_record.western_last_name == individual.western_last_name
        {
            // This should already set from the previous result, but its in here to potentially
            // prevent any weird state bugs.
            individual.object_uuid = matched_record.uuid;
            return Ok(());
        }
    };
    let first_name = &*individual.western_first_name;
    let last_name = &*individual.western_last_name;
    let match_on_first_and_last_name = query!(
        "SELECT * FROM public.humans WHERE western_first_name=$1 AND western_last_name = $2",
        first_name,
        last_name
    )
    .fetch_optional(pool)
    .await?;
    if let Some(matched_record) = match_on_first_and_last_name {
        individual.object_uuid = matched_record.uuid;
        let orig_email_length = matched_record.contact_emails.len();
        let orig_phone_length = matched_record.contact_phone_numbers.len();

        let mut email_set: BTreeSet<String> = matched_record.contact_emails.into_iter().collect();
        let mut phone_set: BTreeSet<String> =
            matched_record.contact_phone_numbers.into_iter().collect();

        // Add individual's contacts (automatically deduplicated)
        email_set.extend(individual.contact_emails.iter().cloned());
        phone_set.extend(individual.contact_phone_numbers.iter().cloned());

        // Convert back to sorted Vec
        let merged_emails: Vec<String> = email_set.into_iter().collect();
        let merged_phones: Vec<String> = phone_set.into_iter().collect();

        // Update database with merged contact info
        if merged_emails.len() != orig_email_length || merged_phones.len() != orig_phone_length {
            sqlx::query!(
                "UPDATE humans SET contact_emails = $1, contact_phone_numbers = $2 WHERE uuid = $3",
                &merged_emails,
                &merged_phones,
                matched_record.uuid
            )
            .execute(pool)
            .await?;
        };

        return Ok(());
    };
    // At this point a new object is very unlikely to exist, so go ahead and add a new object.
    if individual.object_uuid.is_nil() {
        individual.object_uuid = Uuid::new_v4();
    };
    let name = format!(
        "{} {}",
        individual.western_first_name, individual.western_last_name
    );
    let contact_emails = &individual.contact_emails;
    let contact_phones = &individual.contact_phone_numbers;

    sqlx::query!(
        "INSERT INTO humans (uuid, name, western_first_name, western_last_name, contact_emails, contact_phone_numbers) VALUES ($1, $2, $3, $4, $5, $6)",
        individual.object_uuid,
        name,
        individual.western_first_name,
        individual.western_last_name,
        contact_emails,
        contact_phones
    )
    .execute(pool)
    .await?;

    Ok(())
}

// Go ahead and write the same function for an organization

async fn associate_organization_with_name(
    org: &mut ProcessedGenericOrganization,
    pgpool: &PgPool,
) -> Result<(), anyhow::Error> {
    if !org.object_uuid.is_nil() {
        let org_id = org.object_uuid;
        let match_on_uuid = query!("SELECT * FROM public.organizations WHERE uuid=$1", org_id)
            .fetch_optional(pgpool)
            .await?;
        if let Some(matched_record) = match_on_uuid
            && matched_record.name == org.truncated_org_name
        {
            org.object_uuid = matched_record.uuid;
            return Ok(());
        }
    };
    let org_name = org.truncated_org_name.as_str();

    let match_on_org_name = query!("SELECT * FROM public.organizations WHERE name=$1", org_name)
        .fetch_optional(pgpool)
        .await?;
    if let Some(matched_record) = match_on_org_name {
        org.object_uuid = matched_record.uuid;
        return Ok(());
    }

    if org.object_uuid.is_nil() {
        org.object_uuid = Uuid::new_v4();
    }

    sqlx::query!(
        "INSERT INTO organizations (uuid, name, aliases, description, artifical_person_type, org_suffix) VALUES ($1, $2, $3, $4, $5, $6)",
        org.object_uuid,
        org.truncated_org_name,
        &vec![org.truncated_org_name.clone()],
        "",
        "organization",
        ""
    )
    .execute(pgpool)
    .await?;

    Ok(())
}

async fn upload_docket_party_human_connection(
    upload_party: &ProcessedGenericHuman,
    parent_docket: &ProcessedGenericDocket,
    pool: &PgPool,
) -> Result<(), anyhow::Error> {
    if upload_party.object_uuid.is_nil() {
        bail!("Uploading party must have a non nil uuid.")
    }
    if parent_docket.object_uuid.is_nil() {
        bail!("Uploading docket must have a non nil uuid.")
    }

    let party_email = upload_party
        .contact_emails
        .get(0)
        .map(|s| s.as_str())
        .unwrap_or("");
    let party_phone = upload_party
        .contact_phone_numbers
        .get(0)
        .map(|s| s.as_str())
        .unwrap_or("");

    sqlx::query!(
        "INSERT INTO individual_offical_party_to_docket (docket_uuid, individual_uuid, party_email_contact, party_phone_contact) VALUES ($1, $2, $3, $4)",
        parent_docket.object_uuid,
        upload_party.object_uuid,
        party_email,
        party_phone
    )
    .execute(pool)
    .await?;

    Ok(())
}

async fn upload_filling_organization_author(
    upload_org_author: &ProcessedGenericOrganization,
    filling: &ProcessedGenericFiling,
    pool: &PgPool,
) -> Result<(), anyhow::Error> {
    if upload_org_author.object_uuid.is_nil() {
        bail!("Uploading filling author must have a non nil uuid.")
    }
    if filling.object_uuid.is_nil() {
        bail!("Uploading filling must have a non nil uuid.")
    }
    let org_uuid = upload_org_author.object_uuid;
    let filling_uuid = filling.object_uuid;

    sqlx::query!(
            "INSERT INTO fillings_on_behalf_of_org_relation (author_organization_uuid, filling_uuid) VALUES ($1, $2)",
            org_uuid,
            filling_uuid
        )
        .execute(pool)
        .await?;
    Ok(())
}

async fn upload_filling_human_author(
    upload_author: &ProcessedGenericHuman,
    filling: &ProcessedGenericFiling,
    pool: &PgPool,
) -> Result<(), anyhow::Error> {
    if upload_author.object_uuid.is_nil() {
        bail!("Uploading filling author must have a non nil uuid.")
    }
    if filling.object_uuid.is_nil() {
        bail!("Uploading filling must have a non nil uuid.")
    }

    sqlx::query!(
        "INSERT INTO fillings_filed_by_individual (human_uuid, filling_uuid) VALUES ($1, $2)",
        upload_author.object_uuid,
        filling.object_uuid
    )
    .execute(pool)
    .await?;

    Ok(())
}
