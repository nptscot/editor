use anyhow::Result;
use enum_map::Enum;
use geojson::{Feature, FeatureCollection, Geometry};
use graph::{Road, RoadID};
use serde::Serialize;

use crate::{InfraType, MapModel};

#[derive(Clone, Copy, Debug, PartialEq, Enum, Serialize)]
pub enum LevelOfService {
    High,
    Medium,
    Low,
    ShouldNotBeUsed,
}

impl MapModel {
    pub fn render_level_of_service(&self) -> Result<String> {
        let mut features = Vec::new();
        for (idx, road) in self.graph.roads.iter().enumerate() {
            let id = RoadID(idx);

            let mut f = Feature::from(Geometry::from(
                &self.graph.mercator.to_wgs84(&road.linestring),
            ));
            f.set_property("los", serde_json::to_value(self.los[idx])?);
            f.set_property("infra_type", serde_json::to_value(self.get_infra_type(id))?);
            f.set_property("traffic", self.traffic_volumes[idx]);
            f.set_property("speed", self.speeds[idx]);
            features.push(f);
        }

        Ok(serde_json::to_string(&FeatureCollection {
            features,
            bbox: None,
            foreign_members: None,
        })?)
    }

    // TODO Implement directly from
    // https://www.transport.gov.scot/media/50323/cycling-by-design-update-2019-final-document-15-september-2021-1.pdf?
    pub fn calculate_level_of_service(&self, r: RoadID) -> LevelOfService {
        let infra_type = self.get_infra_type(r);
        let speed = self.speeds[r.0];
        let traffic = self.traffic_volumes[r.0];
        // TODO Total placeholder
        match infra_type {
            // TODO The rest of these are still placeholder; osmactive isn't implemented in terms of
            // these categories
            InfraType::SegregatedWide => LevelOfService::High,
            InfraType::OffRoad => LevelOfService::High,
            InfraType::SegregatedNarrow => LevelOfService::Medium,
            InfraType::SharedFootway => LevelOfService::Medium,
            InfraType::CycleLane => LevelOfService::Low,
            // Treat Unknown like MixedTraffic, or like CycleLane?
            InfraType::MixedTraffic | InfraType::Unknown => {
                if speed <= 20 && traffic < 2000 {
                    LevelOfService::High
                } else if speed == 30 && traffic < 1000 {
                    LevelOfService::High
                } else if speed <= 20 && traffic < 4000 {
                    LevelOfService::Medium
                } else if speed == 30 && traffic < 2000 {
                    LevelOfService::Medium
                } else if speed == 40 && traffic < 1000 {
                    LevelOfService::Medium
                } else if speed <= 30 {
                    LevelOfService::Low
                } else if speed == 40 && traffic < 2000 {
                    LevelOfService::Low
                } else if speed == 60 && traffic < 1000 {
                    LevelOfService::Low
                } else {
                    LevelOfService::ShouldNotBeUsed
                }
            }
        }
    }
}

// TODO Unit test
pub fn get_speed_mph(road: &Road) -> usize {
    if road.osm_tags.is("maxspeed", "national") {
        return if road
            .osm_tags
            .is_any("highway", vec!["motorway", "motorway_link"])
        {
            70
        } else {
            60
        };
    }

    if let Some(maxspeed) = road.osm_tags.get("maxspeed") {
        if let Some(mph) = maxspeed
            .strip_suffix(" mph")
            .and_then(|x| x.parse::<usize>().ok())
        {
            return mph;
        }
    }

    match road.osm_tags.get("highway").unwrap().as_str() {
        "residential" | "service" | "unclassified" => 20,
        "tertiary" | "tertiary_link" | "secondary" | "secondary_link" => 30,
        "primary" | "primary_link" => 40,
        "trunk" | "trunk_link" => 60,
        x => {
            error!("get_speed_mph hit unknown highway {x}");
            30
        }
    }
}
