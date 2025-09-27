-- Truncate all tables in the correct order to handle foreign key constraints
-- Relation tables first
TRUNCATE public.docket_petitioned_by_org CASCADE;
TRUNCATE public.fillings_filed_by_individual CASCADE;
TRUNCATE public.fillings_filed_by_org_relation CASCADE;
TRUNCATE public.fillings_on_behalf_of_org_relation CASCADE;
TRUNCATE public.individual_offical_party_to_docket CASCADE;

-- Child tables
TRUNCATE public.attachments CASCADE;

-- Parent tables
TRUNCATE public.fillings CASCADE;
TRUNCATE public.organizations CASCADE;
TRUNCATE public.humans CASCADE;
TRUNCATE public.dockets CASCADE;