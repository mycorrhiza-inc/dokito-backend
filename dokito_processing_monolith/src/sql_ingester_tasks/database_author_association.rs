use anyhow::bail;
use dokito_types::processed::{
    ProcessedGenericDocket, ProcessedGenericFiling, ProcessedGenericHuman,
    ProcessedGenericOrganization,
};
use sqlx::{PgPool, query};
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
        let contact_emails = matched_record.contact_emails;
        let contact_phones = matched_record.contact_phone_numbers;
        let individual_contact_emails = individual.contact_emails;
        let individual_contact_phones = individual.contact_phone_numbers;
        // go ahead and sort the matched contact emails and phones alphabetically and add any that
        // are missing then upload the results that are missing to the database. (DO NOT ADDTHE
        // STUFF FROM POSTGRES TO THE MAIN OBJECT.)
        return Ok(());
    };
    // At this point a new object is very unlikely to exist, so go ahead and add a new object.
    if individual.object_uuid.is_nil() {
        individual.object_uuid = Uuid::new_v4();
    };
    // Go ahead and insert a new individual into postgres using all the data that already exists in
    // this one object.
    todo!()
}

// Go ahead and write the same function for an organization

async fn associate_organization_with_name(
    org: &mut ProcessedGenericOrganization,
    pgpool: &PgPool,
) -> Result<(), anyhow::Error> {
    if !org.object_uuid.is_nil() {
        let org_id = org.object_uuid;
        let match_on_uuid = todo!();
        if let Some(matched_record) = match_on_uuid {
            org.object_uuid = matched_record.uuid;
            return Ok(());
        }
    };
    let org_name = org.truncated_org_name.as_str();

    let match_on_org_name = todo!();
    if Some(matched_record) = match_on_org_name {
        org.object_uuid = matched_record.uuid;
        Ok(())
    }
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
    todo!()
}

async fn upload_filling_organization_author(
    upload_org_author: &ProcessedGenericHuman,
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

    sqlx::query!(
            "INSERT INTO fillings_filed_by_org_relation (author_individual_uuid, filling_uuid) VALUES ($1, $2)",
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
    todo!()
}
//     for indiv_author in filling.individual_authors.iter() {
//         let org_uuid = fetch_or_insert_new_orgname(indiv_author, pool).await?;
//
//         sqlx::query!(
//             "INSERT INTO fillings_filed_by_org_relation (author_individual_uuid, filling_uuid) VALUES ($1, $2)",
//             org_uuid,
//             filling_uuid
//         )
//         .execute(pool)
//         .await?;
//     }
//
//     for org_author in filling.organization_authors.iter() {
//         let org_uuid = fetch_or_insert_new_orgname(org_author, pool).await?;
//         sqlx::query!(
//             "INSERT INTO fillings_on_behalf_of_org_relation (author_organization_uuid, filling_uuid) VALUES ($1, $2)",
//             org_uuid,
//             filling_uuid
//         )
//         .execute(pool)
//         .await?;
//     }
// }
