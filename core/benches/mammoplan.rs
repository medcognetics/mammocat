use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use criterion::{black_box, criterion_group, criterion_main, Criterion, Throughput};
use dicom_core::value::PrimitiveValue;
use dicom_core::{DataElement, Tag, VR};
use dicom_dictionary_std::{tags, uids};
use dicom_object::{FileMetaTableBuilder, InMemDicomObject};
use mammocat_core::{
    plan_mammography_collection, MammographyPlanOptions, MammographyPlanSelection,
};
use tempfile::{tempdir, TempDir};

const STUDY_UID: &str = "1.2.826.0.1.3680043.10.900.1";
const FFDM_SERIES_UID: &str = "1.2.826.0.1.3680043.10.900.2";
const SYNTH_SERIES_UID: &str = "1.2.826.0.1.3680043.10.900.3";
const DBT_SERIES_UID: &str = "1.2.826.0.1.3680043.10.900.4";
const DIGITAL_MAMMOGRAPHY_SOP_CLASS_UID: &str = "1.2.840.10008.5.1.4.1.1.1.2";
const CT_IMAGE_STORAGE_SOP_CLASS_UID: &str = "1.2.840.10008.5.1.4.1.1.2";
const ROWS: u16 = 4;
const COLUMNS: u16 = 4;
const BYTES_PER_FRAME: usize = ROWS as usize * COLUMNS as usize * 2;
const STANDARD_MAMMOGRAM_COUNT: u64 = 5;
const STANDARD_DBT_SLICE_COUNT: u32 = 24;
const STANDARD_SIDECAR_COUNT: usize = 32;
const LARGE_SIDECAR_COUNT: usize = 2_000;
const LARGE_SPLIT_DBT_SLICE_COUNT: u32 = 512;

struct MammoplanFixture {
    _temp_dir: TempDir,
    root: PathBuf,
}

impl MammoplanFixture {
    fn root(&self) -> &Path {
        &self.root
    }
}

#[derive(Clone, Copy)]
enum FixtureKind {
    TwoDOnly,
    DbtOnly,
    Mixed,
    LargeSidecarTree,
    LargeSplitDbtSeries,
}

fn bench_mammoplan_collection_planning(c: &mut Criterion) {
    let two_d_only = mammoplan_fixture(FixtureKind::TwoDOnly);
    let dbt_only = mammoplan_fixture(FixtureKind::DbtOnly);
    let mixed = mammoplan_fixture(FixtureKind::Mixed);

    let mut group = c.benchmark_group("mammoplan_collection_planning");
    group.sample_size(20);
    group.warm_up_time(Duration::from_millis(500));
    group.measurement_time(Duration::from_secs(3));

    group.bench_function("2d_only", |b| {
        b.iter(|| {
            let report = plan_mammography_collection(
                black_box(two_d_only.root()),
                MammographyPlanOptions {
                    selection: MammographyPlanSelection::include_2d_only(),
                    ..MammographyPlanOptions::default()
                },
            )
            .expect("2D fixture should plan successfully");
            black_box(report.summary);
        });
    });

    group.bench_function("dbt_only", |b| {
        b.iter(|| {
            let report = plan_mammography_collection(
                black_box(dbt_only.root()),
                MammographyPlanOptions {
                    selection: MammographyPlanSelection::dbt_only(),
                    ..MammographyPlanOptions::default()
                },
            )
            .expect("DBT fixture should plan successfully");
            black_box(report.summary);
        });
    });

    group.bench_function("mixed_2d_dbt", |b| {
        b.iter(|| {
            let report = plan_mammography_collection(
                black_box(mixed.root()),
                MammographyPlanOptions::default(),
            )
            .expect("mixed fixture should plan successfully");
            black_box(report.summary);
        });
    });

    group.finish();
}

fn bench_large_mammoplan_collection_planning(c: &mut Criterion) {
    let large_sidecar_tree = mammoplan_fixture(FixtureKind::LargeSidecarTree);
    let large_split_dbt_series = mammoplan_fixture(FixtureKind::LargeSplitDbtSeries);

    let mut group = c.benchmark_group("mammoplan_collection_planning_large");
    group.sample_size(10);
    group.warm_up_time(Duration::from_millis(500));
    group.measurement_time(Duration::from_secs(5));

    group.throughput(Throughput::Elements(
        STANDARD_MAMMOGRAM_COUNT + u64::from(STANDARD_DBT_SLICE_COUNT) + LARGE_SIDECAR_COUNT as u64,
    ));
    group.bench_function("large_sidecar_tree", |b| {
        b.iter(|| {
            let report = plan_mammography_collection(
                black_box(large_sidecar_tree.root()),
                MammographyPlanOptions::default(),
            )
            .expect("large sidecar fixture should plan successfully");
            black_box(report.summary);
        });
    });

    group.throughput(Throughput::Elements(u64::from(LARGE_SPLIT_DBT_SLICE_COUNT)));
    group.bench_function("large_split_dbt_series", |b| {
        b.iter(|| {
            let report = plan_mammography_collection(
                black_box(large_split_dbt_series.root()),
                MammographyPlanOptions {
                    selection: MammographyPlanSelection::dbt_only(),
                    ..MammographyPlanOptions::default()
                },
            )
            .expect("large split-DBT fixture should plan successfully");
            black_box(report.summary);
        });
    });

    group.finish();
}

fn mammoplan_fixture(kind: FixtureKind) -> MammoplanFixture {
    let temp_dir = tempdir().expect("tempdir should be created");
    let root = temp_dir.path().join("study");
    fs::create_dir_all(&root).expect("fixture root should be created");

    if matches!(
        kind,
        FixtureKind::TwoDOnly | FixtureKind::Mixed | FixtureKind::LargeSidecarTree
    ) {
        let views_dir = root.join("2d").join("series");
        fs::create_dir_all(&views_dir).expect("2D fixture directory should be created");
        write_mammogram(
            &views_dir.join("l_mlo_ffdm.dcm"),
            FFDM_SERIES_UID,
            1,
            "L",
            "MLO",
        );
        write_mammogram(
            &views_dir.join("r_mlo_ffdm.dcm"),
            FFDM_SERIES_UID,
            2,
            "R",
            "MLO",
        );
        write_mammogram(
            &views_dir.join("l_cc_ffdm.dcm"),
            FFDM_SERIES_UID,
            3,
            "L",
            "CC",
        );
        write_mammogram(
            &views_dir.join("r_cc_ffdm.dcm"),
            FFDM_SERIES_UID,
            4,
            "R",
            "CC",
        );
        write_synth(
            &views_dir.join("l_mlo_synth.dcm"),
            SYNTH_SERIES_UID,
            1,
            "L",
            "MLO",
        );
    }

    let dbt_slice_count = match kind {
        FixtureKind::TwoDOnly => 0,
        FixtureKind::LargeSplitDbtSeries => LARGE_SPLIT_DBT_SLICE_COUNT,
        _ => STANDARD_DBT_SLICE_COUNT,
    };
    if dbt_slice_count > 0 {
        let dbt_dir = root.join("dbt").join("series");
        fs::create_dir_all(&dbt_dir).expect("DBT fixture directory should be created");
        for frame in 1..=dbt_slice_count {
            write_dbt_slice(&dbt_dir.join(format!("slice_{frame:03}.dcm")), frame);
        }
    }

    let sidecar_count = match kind {
        FixtureKind::LargeSidecarTree => LARGE_SIDECAR_COUNT,
        _ => STANDARD_SIDECAR_COUNT,
    };
    write_sidecars(&root, sidecar_count);

    MammoplanFixture {
        _temp_dir: temp_dir,
        root,
    }
}

fn write_sidecars(root: &Path, count: usize) {
    let sidecar_dir = root.join("reports").join("nested");
    fs::create_dir_all(&sidecar_dir).expect("sidecar directory should be created");
    for index in 0..count {
        fs::write(
            sidecar_dir.join(format!("sidecar_{index:03}.txt")),
            format!("not a dicom file {index}\n"),
        )
        .expect("sidecar should be written");
    }
}

fn write_mammogram(path: &Path, series_uid: &str, instance: u32, laterality: &str, view: &str) {
    let sop_uid = format!("{series_uid}.{instance}");
    let mut obj = InMemDicomObject::new_empty();
    put_str(
        &mut obj,
        tags::SOP_CLASS_UID,
        VR::UI,
        DIGITAL_MAMMOGRAPHY_SOP_CLASS_UID,
    );
    put_str(&mut obj, tags::SOP_INSTANCE_UID, VR::UI, &sop_uid);
    put_str(&mut obj, tags::STUDY_INSTANCE_UID, VR::UI, STUDY_UID);
    put_str(&mut obj, tags::SERIES_INSTANCE_UID, VR::UI, series_uid);
    put_str(&mut obj, tags::MODALITY, VR::CS, "MG");
    put_str(&mut obj, tags::IMAGE_LATERALITY, VR::CS, laterality);
    put_str(&mut obj, tags::VIEW_POSITION, VR::CS, view);
    put_str(
        &mut obj,
        tags::PRESENTATION_INTENT_TYPE,
        VR::CS,
        "FOR PRESENTATION",
    );
    put_str(&mut obj, tags::NUMBER_OF_FRAMES, VR::IS, "1");
    put_image_type(&mut obj, &["ORIGINAL", "PRIMARY"]);
    put_geometry(&mut obj);
    put_pixels(&mut obj, vec![0_u8; BYTES_PER_FRAME]);
    write_file(path, obj, DIGITAL_MAMMOGRAPHY_SOP_CLASS_UID, &sop_uid);
}

fn write_synth(path: &Path, series_uid: &str, instance: u32, laterality: &str, view: &str) {
    let sop_uid = format!("{series_uid}.{instance}");
    let mut obj = InMemDicomObject::new_empty();
    put_str(
        &mut obj,
        tags::SOP_CLASS_UID,
        VR::UI,
        DIGITAL_MAMMOGRAPHY_SOP_CLASS_UID,
    );
    put_str(&mut obj, tags::SOP_INSTANCE_UID, VR::UI, &sop_uid);
    put_str(&mut obj, tags::STUDY_INSTANCE_UID, VR::UI, STUDY_UID);
    put_str(&mut obj, tags::SERIES_INSTANCE_UID, VR::UI, series_uid);
    put_str(&mut obj, tags::MODALITY, VR::CS, "MG");
    put_str(&mut obj, tags::IMAGE_LATERALITY, VR::CS, laterality);
    put_str(&mut obj, tags::VIEW_POSITION, VR::CS, view);
    put_str(
        &mut obj,
        tags::PRESENTATION_INTENT_TYPE,
        VR::CS,
        "FOR PRESENTATION",
    );
    put_str(&mut obj, tags::NUMBER_OF_FRAMES, VR::IS, "1");
    put_image_type(&mut obj, &["DERIVED", "PRIMARY", "TOMO_2D"]);
    put_geometry(&mut obj);
    put_pixels(&mut obj, vec![1_u8; BYTES_PER_FRAME]);
    write_file(path, obj, DIGITAL_MAMMOGRAPHY_SOP_CLASS_UID, &sop_uid);
}

fn write_dbt_slice(path: &Path, frame: u32) {
    let sop_uid = format!("{DBT_SERIES_UID}.{frame}");
    let mut obj = InMemDicomObject::new_empty();
    put_str(
        &mut obj,
        tags::SOP_CLASS_UID,
        VR::UI,
        CT_IMAGE_STORAGE_SOP_CLASS_UID,
    );
    put_str(&mut obj, tags::SOP_INSTANCE_UID, VR::UI, &sop_uid);
    put_str(&mut obj, tags::STUDY_INSTANCE_UID, VR::UI, STUDY_UID);
    put_str(&mut obj, tags::SERIES_INSTANCE_UID, VR::UI, DBT_SERIES_UID);
    put_str(&mut obj, tags::MODALITY, VR::CS, "CT");
    put_str(&mut obj, tags::IMAGE_LATERALITY, VR::CS, "L");
    put_str(&mut obj, tags::VIEW_POSITION, VR::CS, "MLO");
    put_str(&mut obj, tags::SERIES_DESCRIPTION, VR::LO, "TOMO L MLO");
    put_str(&mut obj, tags::NUMBER_OF_FRAMES, VR::IS, "1");
    put_str(&mut obj, tags::INSTANCE_NUMBER, VR::IS, &frame.to_string());
    put_image_position(&mut obj, frame);
    put_image_type(&mut obj, &["DERIVED", "PRIMARY", "TOMO", "LEFT"]);
    put_geometry(&mut obj);
    put_pixels(&mut obj, vec![frame as u8; BYTES_PER_FRAME]);
    write_file(path, obj, CT_IMAGE_STORAGE_SOP_CLASS_UID, &sop_uid);
}

fn put_image_type(obj: &mut InMemDicomObject, values: &[&str]) {
    obj.put(DataElement::new(
        tags::IMAGE_TYPE,
        VR::CS,
        PrimitiveValue::Strs(
            values
                .iter()
                .map(|value| (*value).to_string())
                .collect::<Vec<_>>()
                .into(),
        ),
    ));
}

fn put_image_position(obj: &mut InMemDicomObject, frame: u32) {
    obj.put(DataElement::new(
        tags::IMAGE_POSITION_PATIENT,
        VR::DS,
        PrimitiveValue::Strs(vec!["0".to_string(), "0".to_string(), frame.to_string()].into()),
    ));
}

fn put_geometry(obj: &mut InMemDicomObject) {
    put_u16(obj, tags::ROWS, ROWS);
    put_u16(obj, tags::COLUMNS, COLUMNS);
    put_u16(obj, tags::SAMPLES_PER_PIXEL, 1);
    put_str(obj, tags::PHOTOMETRIC_INTERPRETATION, VR::CS, "MONOCHROME2");
    put_u16(obj, tags::BITS_ALLOCATED, 16);
    put_u16(obj, tags::BITS_STORED, 12);
    put_u16(obj, tags::HIGH_BIT, 11);
    put_u16(obj, tags::PIXEL_REPRESENTATION, 0);
}

fn put_pixels(obj: &mut InMemDicomObject, pixel_data: Vec<u8>) {
    obj.put(DataElement::new(
        tags::PIXEL_DATA,
        VR::OW,
        PrimitiveValue::from(pixel_data),
    ));
}

fn put_str(obj: &mut InMemDicomObject, tag: Tag, vr: VR, value: &str) {
    obj.put(DataElement::new(tag, vr, PrimitiveValue::from(value)));
}

fn put_u16(obj: &mut InMemDicomObject, tag: Tag, value: u16) {
    obj.put(DataElement::new(tag, VR::US, PrimitiveValue::from(value)));
}

fn write_file(
    path: &Path,
    obj: InMemDicomObject,
    media_storage_sop_class_uid: &str,
    media_storage_sop_instance_uid: &str,
) {
    let file_obj = obj
        .with_meta(
            FileMetaTableBuilder::new()
                .transfer_syntax(uids::EXPLICIT_VR_LITTLE_ENDIAN)
                .media_storage_sop_class_uid(media_storage_sop_class_uid)
                .media_storage_sop_instance_uid(media_storage_sop_instance_uid),
        )
        .expect("DICOM file meta should be valid");
    file_obj
        .write_to_file(path)
        .expect("DICOM fixture should be written");
}

criterion_group!(
    benches,
    bench_mammoplan_collection_planning,
    bench_large_mammoplan_collection_planning
);
criterion_main!(benches);
