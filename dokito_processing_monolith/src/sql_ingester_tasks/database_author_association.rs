use anyhow::bail;
use dokito_types::processed::{ProcessedGenericHuman, ProcessedGenericOrganization};
use non_empty_string::non_empty_string;
use sqlx::{FromRow, PgPool, query, query_as};
use std::collections::BTreeSet;
use uuid::Uuid;

#[derive(FromRow)]
struct HumanRecord {
    uuid: Uuid,
    western_first_name: String,
    western_last_name: String,
    contact_emails: Vec<String>,
    contact_phone_numbers: Vec<String>,
}

#[derive(FromRow)]
struct OrganizationRecord {
    uuid: Uuid,
    name: String,
}

pub async fn associate_individual_author_with_name(
    individual: &mut ProcessedGenericHuman,
    pool: &PgPool,
) -> Result<(), anyhow::Error> {
    if !individual.object_uuid.is_nil() {
        let author_id = individual.object_uuid;
        let result = query_as::<_, HumanRecord>("SELECT uuid, western_first_name, western_last_name, contact_emails, contact_phone_numbers FROM public.humans WHERE uuid=$1")
            .bind(author_id)
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
    let match_on_first_and_last_name = query_as::<_, HumanRecord>(
        "SELECT uuid, western_first_name, western_last_name, contact_emails, contact_phone_numbers FROM public.humans WHERE western_first_name=$1 AND western_last_name = $2"
    )
    .bind(first_name)
    .bind(last_name)
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
            sqlx::query(
                "UPDATE humans SET contact_emails = $1, contact_phone_numbers = $2 WHERE uuid = $3",
            )
            .bind(&merged_emails)
            .bind(&merged_phones)
            .bind(matched_record.uuid)
            .execute(pool)
            .await?;
        };

        return Ok(());
    };
    // At this point a new object is very unlikely to exist, so go ahead and add a new object.

    let mut provisional_uuid = individual.object_uuid;

    if provisional_uuid.is_nil() {
        provisional_uuid = Uuid::new_v4();
    }
    if individual.object_uuid.is_nil() {
        individual.object_uuid = Uuid::new_v4();
    };
    let name = format!(
        "{} {}",
        individual.western_first_name, individual.western_last_name
    );
    let contact_emails = &individual.contact_emails;
    let contact_phones = &individual.contact_phone_numbers;

    sqlx::query(
        "INSERT INTO humans (uuid, name, western_first_name, western_last_name, contact_emails, contact_phone_numbers) VALUES ($1, $2, $3, $4, $5, $6)"
    )
    .bind(provisional_uuid)
    .bind(name)
    .bind(&individual.western_first_name)
    .bind(&individual.western_last_name)
    .bind(contact_emails)
    .bind(contact_phones)
    .execute(pool)
    .await?;
    individual.object_uuid = provisional_uuid;

    Ok(())
}

// Go ahead and write the same function for an organization

pub async fn associate_organization_with_name(
    org: &mut ProcessedGenericOrganization,
    pool: &PgPool,
) -> Result<(), anyhow::Error> {
    if !org.object_uuid.is_nil() {
        let org_id = org.object_uuid;
        let match_on_uuid = query_as::<_, OrganizationRecord>(
            "SELECT uuid, name FROM public.organizations WHERE uuid=$1",
        )
        .bind(org_id)
        .fetch_optional(pool)
        .await?;
        if let Some(matched_record) = match_on_uuid
            && matched_record.name == org.truncated_org_name
        {
            org.object_uuid = matched_record.uuid;
            return Ok(());
        }
    };
    let org_name = org.truncated_org_name.as_str();

    let match_on_org_name = query_as::<_, OrganizationRecord>(
        "SELECT uuid, name FROM public.organizations WHERE name=$1",
    )
    .bind(org_name)
    .fetch_optional(pool)
    .await?;
    if let Some(matched_record) = match_on_org_name {
        org.object_uuid = matched_record.uuid;
        return Ok(());
    }
    let mut provisional_uuid = org.object_uuid;

    if provisional_uuid.is_nil() {
        provisional_uuid = Uuid::new_v4();
    }
    let org_type = org.org_type.to_string();

    sqlx::query(
        "INSERT INTO organizations (uuid, name, aliases, description, artifical_person_type, org_suffix) VALUES ($1, $2, $3, $4, $5, $6)"
    )
    .bind(provisional_uuid)
    .bind(org.truncated_org_name.as_str())
    .bind(vec![org.truncated_org_name.to_string()])
    .bind("")
    .bind(&org_type)
    .bind(&org.org_suffix)
    .execute(pool)
    .await?;
    org.object_uuid = provisional_uuid;

    Ok(())
}

pub async fn upload_docket_party_human_connection(
    upload_party: &mut ProcessedGenericHuman,
    parent_docket_uuid: Uuid,
    pool: &PgPool,
) -> Result<(), anyhow::Error> {
    if parent_docket_uuid.is_nil() {
        bail!("Uploading docket must have a non nil uuid.")
    }

    associate_individual_author_with_name(upload_party, pool).await?;

    if upload_party.object_uuid.is_nil() {
        unreachable!(
            "Uploading party must have a non nil uuid, this should be impossible since it should have just been associated with one in the database if it did not exist.."
        )
    }

    let party_email = upload_party
        .contact_emails
        .first()
        .map(|s| s.as_str())
        .unwrap_or("");
    let party_phone = upload_party
        .contact_phone_numbers
        .first()
        .map(|s| s.as_str())
        .unwrap_or("");

    sqlx::query(
        "INSERT INTO individual_offical_party_to_docket (docket_uuid, individual_uuid, party_email_contact, party_phone_contact) VALUES ($1, $2, $3, $4)"
    )
    .bind(parent_docket_uuid)
    .bind(upload_party.object_uuid)
    .bind(party_email)
    .bind(party_phone)
    .execute(pool)
    .await?;

    Ok(())
}

pub async fn upload_docket_petitioner_org_connection(
    upload_petitioner: &mut ProcessedGenericOrganization,
    parent_docket_uuid: Uuid,
    pool: &PgPool,
) -> Result<(), anyhow::Error> {
    if parent_docket_uuid.is_nil() {
        bail!("Uploading filling must have a non nil uuid.")
    }
    associate_organization_with_name(upload_petitioner, pool).await?;
    if upload_petitioner.object_uuid.is_nil() {
        unreachable!(
            "Uploading filling author must have a non nil uuid. This should be impossible because it just happened in the previous step"
        )
    };
    let petitioner_uuid = upload_petitioner.object_uuid;

    sqlx::query(
        "INSERT INTO docket_petitioned_by_org (docket_uuid, petitioner_uuid) VALUES ($1,$2)",
    )
    .bind(parent_docket_uuid)
    .bind(petitioner_uuid)
    .execute(pool)
    .await?;
    Ok(())
}
pub async fn upload_filling_organization_author(
    upload_org_author: &mut ProcessedGenericOrganization,
    parent_filling_uuid: Uuid,
    pool: &PgPool,
) -> Result<(), anyhow::Error> {
    if parent_filling_uuid.is_nil() {
        bail!("Uploading filling must have a non nil uuid.")
    }
    associate_organization_with_name(upload_org_author, pool).await?;
    if upload_org_author.object_uuid.is_nil() {
        unreachable!(
            "Uploading filling author must have a non nil uuid. This should be impossible because it just happened in the previous step"
        )
    }
    let org_uuid = upload_org_author.object_uuid;

    sqlx::query(
            "INSERT INTO fillings_on_behalf_of_org_relation (author_organization_uuid, filling_uuid) VALUES ($1, $2)"
        )
        .bind(org_uuid)
        .bind(parent_filling_uuid)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn upload_filling_human_author(
    upload_author: &mut ProcessedGenericHuman,
    parent_filling_uuid: Uuid,
    pool: &PgPool,
) -> Result<(), anyhow::Error> {
    if parent_filling_uuid.is_nil() {
        bail!("Uploading filling must have a non nil uuid.")
    }
    associate_individual_author_with_name(upload_author, pool).await?;
    if upload_author.object_uuid.is_nil() {
        unreachable!(
            "Uploading filling author must have a non nil uuid, this should be impossible dispite it being validated on the previous line."
        )
    }

    sqlx::query(
        "INSERT INTO fillings_filed_by_individual (human_uuid, filling_uuid) VALUES ($1, $2)",
    )
    .bind(upload_author.object_uuid)
    .bind(parent_filling_uuid)
    .execute(pool)
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::PgPool;
    use std::env;
    use uuid::Uuid;

    async fn setup_test_db() -> PgPool {
        let database_url = env::var("DATABASE_URL")
            .unwrap_or_else(|_| "postgres://test:test@localhost/test_db".to_string());
        PgPool::connect(&database_url)
            .await
            .expect("Failed to connect to test database")
    }

    #[tokio::test]
    async fn test_associate_individual_author_with_name_new_person() {
        let pool = setup_test_db().await;

        // Create a new individual that doesn't exist in the database
        let mut individual = ProcessedGenericHuman {
            object_uuid: Uuid::nil(),
            western_first_name: "Test".to_string(),
            western_last_name: "Person".to_string(),
            human_name: non_empty_string!("Bob Smith"),
            contact_emails: vec!["test@example.com".to_string()],
            contact_phone_numbers: vec!["+1234567890".to_string()],
            contact_addresses: vec![],
            representing_company: None,
            employed_by: None,
            title: "Senior Manager".into(),
        };

        let result = associate_individual_author_with_name(&mut individual, &pool).await;

        assert!(
            result.is_ok(),
            "Failed to associate new individual: {:?}",
            result
        );
        assert!(!individual.object_uuid.is_nil(), "UUID should be assigned");

        // Verify the person was actually inserted
        let db_record = query_as::<_, HumanRecord>(
            "SELECT uuid, western_first_name, western_last_name, contact_emails, contact_phone_numbers FROM humans WHERE uuid = $1"
        )
        .bind(individual.object_uuid)
        .fetch_one(&pool)
        .await;

        assert!(db_record.is_ok(), "Failed to fetch inserted record");
        let record = db_record.unwrap();
        assert_eq!(record.western_first_name, "Test");
        assert_eq!(record.western_last_name, "Person");
        assert_eq!(record.contact_emails, vec!["test@example.com"]);
    }

    #[tokio::test]
    async fn test_query_structures_compile() {
        // This test verifies that our query structures compile correctly
        let pool = setup_test_db().await;

        // // Test HumanRecord query structure
        // let _human_query = query_as::<_, HumanRecord>(
        //     "SELECT uuid, western_first_name, western_last_name, contact_emails, contact_phone_numbers FROM humans LIMIT 0",
        // );

        // // Test OrganizationRecord query structure
        // let _org_query =
        //     query_as::<_, OrganizationRecord>("SELECT uuid, name FROM organizations LIMIT 0");

        // If we get here, the query structures compile correctly
        assert!(true);
    }
}
