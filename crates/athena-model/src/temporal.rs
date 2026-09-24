use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TimeRel {
    Before,
    After,
    Between,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TimeProperty {
    ObservedAt,
    CreatedAt,
    ModifiedAt,
}

impl Default for TimeProperty {
    fn default() -> Self {
        Self::ObservedAt
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AggrMethod {
    Avg,
    DistinctCount,
    Max,
    Min,
    Sum,
    Stddev,
    Sumsq,
    TotalCount,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TemporalQuery {
    pub timerel: TimeRel,
    pub time_at: DateTime<Utc>,
    pub end_time_at: Option<DateTime<Utc>>,
    pub timeproperty: TimeProperty,
    pub aggr_method: Option<AggrMethod>,
    #[serde(default)]
    pub aggr_methods: Vec<AggrMethod>,
    pub aggr_period_duration: Option<String>,
    pub last_n: Option<usize>,
}
