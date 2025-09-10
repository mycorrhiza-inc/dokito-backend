use crate::processing::process_case;
use crate::s3_stuff::make_s3_client;
use async_trait::async_trait;
use dokito_types::raw::RawDocketWithJurisdiction;
use mycorrhiza_common::tasks::ExecuteUserTask;

#[repr(transparent)]
pub struct ProcessCaseWithoutDownload(pub RawDocketWithJurisdiction);

#[async_trait]
impl ExecuteUserTask for ProcessCaseWithoutDownload {
    async fn execute_task(self: Box<Self>) -> Result<serde_json::Value, serde_json::Value> {
        let s3_client = make_s3_client().await;
        let RawDocketWithJurisdiction {
            docket,
            jurisdiction,
        } = self.0;
        let extra_data = (s3_client, jurisdiction);
        let res = process_case(docket, &extra_data).await;
        match res {
            Ok(proc_docket) => Ok(serde_json::to_value(proc_docket)
                .unwrap_or("Processed Case but could not serialize".into())),
            Err(err) => Err(err.to_string().into()),
        }
    }
    fn get_task_label(&self) -> &'static str {
        "ingest_case_with_jurisdiction_and_download"
    }
    fn get_task_label_static() -> &'static str
    where
        Self: Sized,
    {
        "ingest_case_with_jurisdiction_and_download"
    }
}
