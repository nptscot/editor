use std::collections::HashMap;
use std::fs::File;
use std::io::BufWriter;

use anyhow::{bail, Result};
use clap::Parser;
use gdal::{vector::LayerAccess, Dataset};
use geo::{Distance, Euclidean, FrechetDistance, HausdorffDistance, Geometry, LineString, MultiPolygon};
use graph::{Graph, Timer};
use log::info;
use rstar::{primitives::GeomWithData, RTree, RTreeObject};
use serde::Deserialize;

use backend::{MapMatch, MapModel};

#[derive(Parser)]
struct Args {
    /// Path to a .osm.pbf or .xml file to convert
    #[arg(long)]
    input: String,

    /// Path to GeoJSON file with the boundary to clip the input to
    #[arg(long)]
    boundary: String,

    /// Output file to write
    #[arg(long)]
    output: String,
}

fn main() -> Result<()> {
    simple_logger::init_with_level(log::Level::Info).unwrap();
    let args = Args::parse();

    let mut timer = Timer::new("build model", None);
    let osm_bytes = std::fs::read(&args.input)?;
    let boundary_gj = std::fs::read_to_string(&args.boundary)?;
    let model = create(&osm_bytes, &boundary_gj, &mut timer)?;

    timer.step("writing");
    let writer = BufWriter::new(File::create(&args.output)?);
    bincode::serialize_into(writer, &model)?;

    timer.done();
    Ok(())
}

fn create(input_bytes: &[u8], boundary_gj: &str, timer: &mut Timer) -> Result<MapModel> {
    let graph = Graph::new(
        input_bytes,
        &mut utils::osm2graph::NullReader,
        vec![(
            "bicycle".to_string(),
            Box::new(backend::existing::bicycle_profile),
        )],
        timer,
    )?;
    let boundary_wgs84 = read_multipolygon(boundary_gj)?;

    timer.step("loading OD zones");
    let od_zones = backend::od::Zone::parse_zones(
        std::fs::read_to_string("../data_prep/tmp/zones.geojson")?,
        &boundary_wgs84,
        &graph.mercator,
    )?;

    timer.step("loading desire lines");
    let desire_lines = read_desire_lines_csv("../data_prep/tmp/od.csv", &od_zones)?;

    timer.step("loading schools");
    let schools = backend::places::School::from_gj(
        &std::fs::read_to_string("../data_prep/tmp/schools.geojson")?,
        &boundary_wgs84,
        &graph,
    )?;

    timer.step("loading GPs/hospitals");
    let gp_hospitals = backend::places::GPHospital::from_gj(
        &std::fs::read_to_string("../data_prep/tmp/gp_practices.geojson")?,
        &std::fs::read_to_string("../data_prep/tmp/hospitals.geojson")?,
        &boundary_wgs84,
        &graph,
    )?;

    timer.step("loading town centres");
    let town_centres = backend::places::TownCentre::from_gj(
        &std::fs::read_to_string("../data_prep/tmp/town_centres.geojson")?,
        &boundary_wgs84,
        &graph,
    )?;

    timer.step("loading data zones");
    let data_zones = backend::places::DataZone::from_gj(
        &std::fs::read_to_string("../data_prep/tmp/population.geojson")?,
        &boundary_wgs84,
        &graph,
    )?;

    let traffic_volumes = read_traffic_volumes("../data_prep/tmp/traffic.gpkg", &graph, timer)?;

    let core_network = read_core_network("../data_prep/tmp/core_network.gpkg", &graph, timer)?;

    let precalculated_flows =
        read_precalculated_flows("../data_prep/tmp/combined_network.gpkg", &graph, timer)?;

    Ok(MapModel::create(
        graph,
        boundary_wgs84,
        od_zones,
        desire_lines,
        schools,
        gp_hospitals,
        town_centres,
        data_zones,
        traffic_volumes,
        core_network,
        precalculated_flows,
    ))
}

fn read_multipolygon(gj_string: &str) -> Result<MultiPolygon> {
    let gj: geojson::Feature = gj_string.parse()?;
    if matches!(
        gj.geometry.as_ref().unwrap().value,
        geojson::Value::Polygon(_)
    ) {
        Ok(MultiPolygon(vec![gj.try_into()?]))
    } else {
        Ok(gj.try_into()?)
    }
}

fn read_desire_lines_csv(
    path: &str,
    zones: &HashMap<String, backend::od::Zone>,
) -> Result<Vec<(String, String, usize)>> {
    let mut out = Vec::new();
    for rec in csv::Reader::from_reader(File::open(path)?).deserialize() {
        let row: DesireLineRow = rec?;
        if zones.contains_key(&row.geo_code1) && zones.contains_key(&row.geo_code2) {
            out.push((row.geo_code1, row.geo_code2, row.all));
        }
    }
    Ok(out)
}

#[derive(Deserialize)]
struct DesireLineRow {
    geo_code1: String,
    geo_code2: String,
    all: usize,
}

fn read_traffic_volumes(path: &str, graph: &Graph, timer: &mut Timer) -> Result<Vec<usize>> {
    // Read all relevant lines and make an RTree
    timer.step("read traffic volumes");
    let dataset = Dataset::open(path)?;
    let mut layer = dataset.layer(0)?;
    let b = &graph.mercator.wgs84_bounds;
    layer.set_spatial_filter_rect(b.min().x, b.min().y, b.max().x, b.max().y);

    let mut traffic_links = Vec::new();
    for input in layer.features() {
        let mut geom: LineString = input.geometry().unwrap().to_geo()?.try_into()?;
        graph.mercator.to_mercator_in_place(&mut geom);
        let Some(flows) = input.field_as_integer_by_name("pred_flows")? else {
            // TODO Why missing?
            continue;
        };
        traffic_links.push(GeomWithData::new(geom, flows as usize));
    }
    info!("{} links have pred_flows", traffic_links.len());
    let rtree = RTree::bulk_load(traffic_links);

    // Multiple roads might match to the same traffic link -- dual carriageways, for example.
    // Insist on finding a match for every road.
    //
    // TODO Check results carefully. npt build.R does more work.
    timer.step("match roads to traffic volumes");
    let mut output = Vec::new();
    for road in &graph.roads {
        // The source data excludes these, so we often end up matching nearby main roads with high
        // volume. Force them to 0, so they get a good LoS and don't appear as severances.
        if road.osm_tags.is("highway", "service") {
            output.push(0);
            continue;
        }

        if let Some(link) = rtree
            .locate_in_envelope_intersecting(&road.linestring.envelope())
            .min_by_key(|link| compare_road_geometry(&road.linestring, link.geom()))
        {
            //info!("{} got flow {} with score {} cm", road.way, link.data, compare_road_geometry(&road.linestring, link.geom()));
            output.push(link.data);
        } else {
            //info!("{} didn't match a traffic volume link", road.way);
            output.push(0);
        }
    }
    Ok(output)
}

// The output is whether each road is part of the core network or not
fn read_core_network(path: &str, graph: &Graph, timer: &mut Timer) -> Result<Vec<MapMatch>> {
    // Read all relevant lines and make an RTree
    timer.step("read core network");
    let dataset = Dataset::open(path)?;
    let mut layer = dataset.layer(0)?;
    let b = &graph.mercator.wgs84_bounds;
    layer.set_spatial_filter_rect(b.min().x, b.min().y, b.max().x, b.max().y);

    let mut segments = Vec::new();
    for input in layer.features() {
        let geo = input.geometry().unwrap().to_geo()?;
        match geo {
            Geometry::LineString(mut ls) => {
                graph.mercator.to_mercator_in_place(&mut ls);
                segments.push(ls);
            }
            Geometry::MultiLineString(mls) => {
                for mut ls in mls {
                    graph.mercator.to_mercator_in_place(&mut ls);
                    segments.push(ls);
                }
            }
            _ => bail!("read_core_network found something besides a LS or MLS"),
        }
    }
    let rtree = RTree::bulk_load(segments);

    // Multiple roads might match to the same segment -- dual carriageways, for example. For every
    // road, see if there's any core network segment close enough to it. Note this assumes the core
    // network is split into small segments!
    timer.step("match roads to core network segments");
    let mut output = Vec::new();
    for road in &graph.roads {
        let mut result = MapMatch {
            hausdorff: f64::INFINITY,
            frechet: f64::INFINITY,
            endpt1_diff: f64::INFINITY,
            endpt2_diff: f64::INFINITY,
        };
        for geom in rtree.locate_in_envelope_intersecting(&road.linestring.envelope()) {
            result.hausdorff = result
                .hausdorff
                .min(road.linestring.hausdorff_distance(geom));
            result.frechet = result.frechet.min(road.linestring.frechet_distance(geom));
            let dist1 =
                Euclidean::distance(*road.linestring.coords().next().unwrap(), *geom.coords().next().unwrap());
            let dist2 =
                Euclidean::distance(*road.linestring.coords().last().unwrap(), *geom.coords().last().unwrap());
            result.endpt1_diff = result.endpt1_diff.min(dist1);
            result.endpt2_diff = result.endpt2_diff.min(dist2);
        }

        output.push(result);
    }
    Ok(output)
}

// The output is the Go Dutch totals for all purpose
fn read_precalculated_flows(path: &str, graph: &Graph, timer: &mut Timer) -> Result<Vec<usize>> {
    // Read all relevant lines and make an RTree
    timer.step("read precalculated flows");
    let dataset = Dataset::open(path)?;
    let mut layer = dataset.layer(0)?;
    let b = &graph.mercator.wgs84_bounds;
    layer.set_spatial_filter_rect(b.min().x, b.min().y, b.max().x, b.max().y);

    let mut links = Vec::new();
    for input in layer.features() {
        let mut geom: LineString = input.geometry().unwrap().to_geo()?.try_into()?;
        graph.mercator.to_mercator_in_place(&mut geom);
        // TODO Or quietest?
        let Some(flow) = input.field_as_integer_by_name("all_fastest_bicycle_go_dutch")? else {
            bail!("combined_network is missing all_fastest_bicycle_go_dutch");
        };
        links.push(GeomWithData::new(geom, flow as usize));
    }
    let rtree = RTree::bulk_load(links);

    // Multiple roads might match to the same link -- dual carriageways, for example.
    // Insist on finding a match for every road.
    timer.step("match roads to precalculated flows");
    let mut output = Vec::new();
    for road in &graph.roads {
        // TODO Skip service roads or similar? Check what's in the NPT rnet

        if let Some(link) = rtree
            .locate_in_envelope_intersecting(&road.linestring.envelope())
            .min_by_key(|link| compare_road_geometry(&road.linestring, link.geom()))
        {
            output.push(link.data);
        } else {
            output.push(0);
        }
    }
    Ok(output)
}

// Just sum distance between endpoints
// TODO When the volume links are much longer than OSM, or vice versa, how well does this work?
fn compare_road_geometry(ls1: &LineString, ls2: &LineString) -> usize {
    let dist1 = Euclidean::distance(*ls1.coords().next().unwrap(), *ls2.coords().next().unwrap());
    let dist2 = Euclidean::distance(*ls1.coords().last().unwrap(), *ls2.coords().last().unwrap());
    // cm precision
    ((dist1 + dist2) * 100.0) as usize
}

// TODO Works poorly in many cases
fn road_geometry_similar(ls1: &LineString, ls2: &LineString) -> bool {
    // If both endpoints are within this threshold, accept it
    let tolerance_per_endpt = 10.0;

    let dist1 = Euclidean::distance(*ls1.coords().next().unwrap(), *ls2.coords().next().unwrap());
    let dist2 = Euclidean::distance(*ls1.coords().last().unwrap(), *ls2.coords().last().unwrap());

    dist1 <= tolerance_per_endpt && dist2 <= tolerance_per_endpt
}
