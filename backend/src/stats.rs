use anyhow::Result;
use enum_map::EnumMap;
use graph::Timer;

use crate::{InfraType, MapModel};

impl MapModel {
    /// After any edit, calculate summary stats. Returns JSON.
    pub fn recalculate_stats(&mut self, timer: &mut Timer) -> Result<String> {
        self.recalculate_router(timer);

        timer.step("calculate OD routes and stats");

        let mut count_by_infra: EnumMap<InfraType, usize> = EnumMap::default();
        let mut count_off_network = 0;
        let mut total_count = 0;

        let od = self.od_counts()?;
        for (r, count) in od.counts {
            total_count += count;
            if let Some(infra_type) = self.infra_types[r.0] {
                count_by_infra[infra_type] += count;
            } else {
                count_off_network += count;
            }
        }

        let mut od_percents = serde_json::Map::new();
        od_percents.insert(
            "Off network".to_string(),
            percent(count_off_network, total_count).into(),
        );
        for (infra_type, count) in count_by_infra {
            od_percents.insert(
                format!("{infra_type:?}"),
                percent(count, total_count).into(),
            );
        }

        let mut out = serde_json::Map::new();
        out.insert(
            "od_percents".to_string(),
            serde_json::Value::Object(od_percents),
        );
        out.insert(
            "average_weighted_directness".to_string(),
            od.average_weighted_directness.into(),
        );

        Ok(serde_json::to_string(&out)?)
    }
}

fn percent(x: usize, total: usize) -> f64 {
    if total == 0 {
        0.0
    } else {
        (x as f64) / (total as f64)
    }
}