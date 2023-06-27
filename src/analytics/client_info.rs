use {parquet_derive::ParquetRecordWriter, serde::Serialize, std::sync::Arc};

#[derive(Debug, Clone, Serialize, ParquetRecordWriter)]
#[serde(rename_all = "camelCase")]
pub struct ClientInfo {
    pub project_id: Arc<str>,
    pub account: Arc<str>,
    pub topic: Arc<str>,
    pub registered_at: chrono::NaiveDateTime,
}
