use dokito_types::jurisdictions::JurisdictionInfo;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum FixedJurisdiction {
    NewYorkPuc,
    ColoradoPuc,
    CaliforniaPuc,
    UtahDogmCoal,
}
impl FixedJurisdiction {
    pub fn get_country_code(&self) -> &'static str {
        // explanation for how this country assignment works: https://youtu.be/TmoeZHnOJKA?t=36
        "usa"
    }
    pub fn get_state_code(&self) -> &'static str {
        match self {
            FixedJurisdiction::NewYorkPuc => "ny",
            FixedJurisdiction::ColoradoPuc => "co",
            FixedJurisdiction::CaliforniaPuc => "ca",
            FixedJurisdiction::UtahDogmCoal => "ut",
        }
    }

    pub fn get_jurisdiction_info_name(&self) -> &'static str {
        match self {
            FixedJurisdiction::NewYorkPuc => "ny_puc",
            FixedJurisdiction::ColoradoPuc => "co_puc",
            FixedJurisdiction::CaliforniaPuc => "ca_puc",
            FixedJurisdiction::UtahDogmCoal => "ut_dogm_coal",
        }
    }

    pub fn get_postgres_schema_name(&self) -> &'static str {
        match self {
            FixedJurisdiction::NewYorkPuc => "ny_puc_data",
            FixedJurisdiction::ColoradoPuc => "co_puc_data",
            FixedJurisdiction::CaliforniaPuc => "ca_puc_data",
            FixedJurisdiction::UtahDogmCoal => "ut_dogm_coal_data",
        }
    }
}

const ALL_FIXED_JURISDICTIONS: &[FixedJurisdiction] = &[
    FixedJurisdiction::NewYorkPuc,
    FixedJurisdiction::ColoradoPuc,
    FixedJurisdiction::CaliforniaPuc,
    FixedJurisdiction::UtahDogmCoal,
];
impl TryFrom<&JurisdictionInfo> for FixedJurisdiction {
    type Error = ();

    fn try_from(value: &JurisdictionInfo) -> Result<Self, Self::Error> {
        ALL_FIXED_JURISDICTIONS
            .iter()
            .copied()
            .find(|jur| {
                jur.get_jurisdiction_info_name() == value.jurisdiction
                    && jur.get_state_code() == value.state
                    && jur.get_country_code() == value.country
            })
            .ok_or(())
    }
}

impl From<FixedJurisdiction> for JurisdictionInfo {
    fn from(value: FixedJurisdiction) -> Self {
        JurisdictionInfo {
            country: value.get_country_code().to_string(),
            state: value.get_state_code().to_string(),
            jurisdiction: value.get_jurisdiction_info_name().to_string(),
        }
    }
}
