use std::collections::HashSet;

use anyhow::Result;
use enum_map::EnumMap;
use geo::LineString;
use geojson::{feature::Id, Feature, GeoJson};
use graph::{Graph, PathStep, Position, RoadID};
use serde::Serialize;

use crate::join_lines::{Dir, KeyedLineString};
use crate::{InfraType, LevelOfService, MapModel, Route, Tier};

impl MapModel {
    /// Returns the route ID
    pub fn set_route(&mut self, edit_id: Option<usize>, route: Route) -> Result<usize> {
        let original = if let Some(id) = edit_id {
            match self.routes.remove(&id) {
                Some(route) => Some(route),
                None => bail!("Unknown route {id}"),
            }
        } else {
            None
        };

        // Check for overlaps
        let used_roads = self.used_roads();
        for r in &route.roads {
            if used_roads.contains(r) {
                // Restore the original
                if let (Some(id), Some(route)) = (edit_id, original) {
                    self.routes.insert(id, route);
                }

                bail!("Another route already crosses the same road {r:?}");
            }
        }

        let id = match edit_id {
            Some(id) => id,
            None => {
                let id = self.id_counter;
                self.id_counter += 1;
                id
            }
        };
        self.routes.insert(id, route);
        self.recalculate_after_edits();
        Ok(id)
    }

    pub fn delete_route(&mut self, id: usize) -> Result<()> {
        if self.routes.remove(&id).is_some() {
            self.recalculate_after_edits();
            return Ok(());
        }
        bail!("Unknown route {id}");
    }

    pub fn clear_all_routes(&mut self) {
        self.routes.clear();
        self.id_counter = 0;
        self.recalculate_after_edits();
    }

    pub fn to_routes_gj(&self) -> GeoJson {
        let mut features = Vec::new();
        for (id, route) in &self.routes {
            let mut f = route.feature.clone();
            f.id = Some(Id::Number((*id).into()));
            f.set_property("name", route.name.clone());
            f.set_property("notes", route.notes.clone());
            f.set_property(
                "infra_type",
                serde_json::to_value(&route.infra_type).unwrap(),
            );
            f.set_property("tier", serde_json::to_value(&route.tier).unwrap());
            features.push(f);
        }
        GeoJson::from(features)
    }

    /// Returns the number of edits
    pub fn import_existing_routes(&mut self) -> usize {
        let used_roads = self.used_roads();
        let mut imports = Vec::new();
        for (idx, road) in self.graph.roads.iter().enumerate() {
            let road_id = RoadID(idx);
            if used_roads.contains(&road_id) {
                continue;
            }
            let Some(infra_type) = crate::existing::classify(&road.osm_tags) else {
                continue;
            };
            if !matches!(
                infra_type,
                InfraType::SegregatedWide | InfraType::OffRoad | InfraType::SegregatedNarrow,
            ) {
                continue;
            }
            imports.push((road_id, infra_type));
        }

        // TODO Can we detect the tier, or should this entire "import" feature go away and be
        // user-driven?
        self.import_roads(imports, Tier::LocalAccess)
    }

    /// Returns the number of edits
    pub fn import_core_network(&mut self) -> usize {
        let used_roads = self.used_roads();
        let mut imports: EnumMap<Tier, Vec<(RoadID, InfraType)>> = EnumMap::default();

        for idx in 0..self.graph.roads.len() {
            let road_id = RoadID(idx);
            if used_roads.contains(&road_id) {
                continue;
            }
            if let Some(tier) = self.core_network[idx] {
                // TODO What type?
                imports[tier].push((road_id, InfraType::SegregatedNarrow));
            }
        }

        let mut edits = 0;
        for (tier, roads) in imports {
            edits += self.import_roads(roads, tier)
        }
        edits
    }

    /// Split a route into sections, returning a FeatureCollection
    pub fn autosplit_route(&self, route: Vec<(RoadID, bool)>) -> Result<String> {
        let used_roads = self.used_roads();

        // Split when:
        // - the auto-recommended infrastructure type changes
        // - the route crosses something existing
        #[derive(PartialEq)]
        enum Case {
            AlreadyExists,
            New(Option<InfraType>),
        }
        let case = |(r, _)| {
            if used_roads.contains(&r) {
                Case::AlreadyExists
            } else {
                Case::New(self.best_infra_type(r))
            }
        };

        let mut sections = Vec::new();
        for roads in route.chunk_by(|a, b| case(*a) == case(*b)) {
            let c = case(roads[0]);
            let mut f = self
                .graph
                .mercator
                .to_wgs84_gj(&glue_route(&self.graph, roads));
            match c {
                Case::AlreadyExists => {
                    f.set_property("kind", "overlap");
                }
                Case::New(infra_type) => {
                    f.set_property("kind", "new");
                    f.set_property("infra_type", serde_json::to_value(&infra_type).unwrap());
                }
            }
            sections.push(f);
        }
        Ok(serde_json::to_string(&GeoJson::from(sections))?)
    }

    fn import_roads(&mut self, imports: Vec<(RoadID, InfraType)>, tier: Tier) -> usize {
        // Create individual segments to import
        let mut pieces = Vec::new();
        for (id, infra_type) in imports {
            pieces.push(KeyedLineString {
                linestring: self.graph.roads[id.0].linestring.clone(),
                ids: vec![(id, Dir::Forwards)],
                key: infra_type,
            });
        }

        // Group them in hopefully meaningful chunks
        // TODO Could try more aggressive joining after this, but this one seems to work fine so
        // far. Although oddly it seems to handle more than just degree 2...
        pieces = crate::join_lines::collapse_degree_2(pieces);
        let changes = pieces.len();

        for line in pieces {
            let route = Route {
                feature: make_route_snapper_feature(&self.graph, &line.ids, &line.linestring),
                // Pick the first name
                // TODO Does this short-circuit?
                name: line
                    .ids
                    .iter()
                    .filter_map(|(r, _)| self.graph.roads[r.0].osm_tags.get("name").cloned())
                    .next()
                    .unwrap_or_else(String::new),
                notes: "imported from existing network".to_string(),
                roads: line.ids.into_iter().map(|(r, _)| r).collect(),
                infra_type: line.key,
                tier,
            };
            let route_id = self.id_counter;
            self.id_counter += 1;
            self.routes.insert(route_id, route);
        }

        self.recalculate_after_edits();
        changes
    }

    fn used_roads(&self) -> HashSet<RoadID> {
        self.routes
            .values()
            .flat_map(|route| route.roads.clone())
            .collect()
    }

    // TODO Use CbD guidance. Simple for now
    // This assumes this road doesn't have anything set yet, and so its LoS isn't based on an
    // InfraType already
    fn best_infra_type(&self, r: RoadID) -> Option<InfraType> {
        match self.los[r.0] {
            // Already fine
            LevelOfService::High => None,
            LevelOfService::Medium => Some(InfraType::SegregatedNarrow),
            LevelOfService::Low => Some(InfraType::SegregatedWide),
            // TODO The user drew a route here, so what should we recommend?
            LevelOfService::ShouldNotBeUsed => Some(InfraType::SegregatedWide),
        }
    }
}

// Mimic enough of what the route snapper creates, so the segment can be edited in the web app
fn make_route_snapper_feature(
    graph: &Graph,
    ids: &Vec<(RoadID, Dir)>,
    linestring: &LineString,
) -> Feature {
    let mut intersections = Vec::new();
    for (r, dir) in ids {
        let road = &graph.roads[r.0];
        if matches!(dir, Dir::Forwards) {
            intersections.push(road.src_i);
            intersections.push(road.dst_i);
        } else {
            intersections.push(road.dst_i);
            intersections.push(road.src_i);
        }
    }
    intersections.dedup();

    let mut f = graph.mercator.to_wgs84_gj(linestring);

    // We don't know what waypoints we could leave out without doing some kind of iterative
    // approach. For now, just include all of them.
    let waypoints = intersections
        .iter()
        .map(|i| {
            let pt = graph.mercator.to_wgs84(&graph.intersections[i.0].point);
            serde_json::to_value(&RouteWaypoint {
                lon: trim_lon_lat(pt.x()),
                lat: trim_lon_lat(pt.y()),
                snapped: true,
            })
            .unwrap()
        })
        .collect();
    f.set_property("waypoints", serde_json::Value::Array(waypoints));

    let full_path = intersections
        .iter()
        .map(|i| {
            serde_json::to_value(&JsonNode {
                snapped: i.0 as u32,
            })
            .unwrap()
        })
        .collect();
    f.set_property("full_path", serde_json::Value::Array(full_path));

    f
}

#[derive(Serialize)]
struct RouteWaypoint {
    lon: f64,
    lat: f64,
    snapped: bool,
}

#[derive(Serialize)]
struct JsonNode {
    snapped: u32,
}

// Per https://datatracker.ietf.org/doc/html/rfc7946#section-11.2, 6 decimal places (10cm) is
// plenty of precision
fn trim_lon_lat(x: f64) -> f64 {
    (x * 10e6).round() / 10e6
}

// TODO Upstream to graph
fn glue_route(graph: &Graph, roads: &[(RoadID, bool)]) -> LineString {
    graph::Route {
        start: start_pos(roads[0], graph),
        end: end_pos(*roads.last().unwrap(), graph),
        steps: roads
            .into_iter()
            .cloned()
            .map(|(road, forwards)| PathStep::Road { road, forwards })
            .collect(),
    }
    .linestring(graph)
}

// TODO Upstream to graph
fn start_pos((r, forwards): (RoadID, bool), graph: &Graph) -> Position {
    let road = &graph.roads[r.0];
    Position {
        road: r,
        fraction_along: if forwards { 0.0 } else { 1.0 },
        intersection: if forwards { road.src_i } else { road.dst_i },
    }
}

fn end_pos((road, forwards): (RoadID, bool), graph: &Graph) -> Position {
    start_pos((road, !forwards), graph)
}
