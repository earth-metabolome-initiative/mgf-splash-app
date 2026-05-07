//! Core MGF parsing and SPLASH reporting types for the web app and worker.

use std::{
    collections::BTreeMap,
    error::Error,
    fmt::{Display, Formatter, Result as FmtResult, Write as _},
};

use mascot_rs::prelude::{MGFIter, MascotGenericFormat, Spectrum as MascotSpectrum};
use mass_spectrometry::prelude::{
    GenericSpectrum, SpectrumMut, SpectrumSplash as LatestSpectrumSplash,
};
use serde::{Deserialize, Serialize};

/// Example MGF document used by the app's sample button.
pub const SAMPLE_MGF: &str = r"BEGIN IONS
TITLE=Two peak SPLASH sanity check
SOURCE=mass-spectrometry-traits README
FEATURE_ID=1
PEPMASS=250.0
CHARGE=1
RTINSECONDS=10.0
MSLEVEL=2
SCANS=1
100.0 10.0
200.0 20.0
END IONS

BEGIN IONS
TITLE=Aspirin reference spectrum
SOURCE=mass-spectrometry-traits/src/traits/reference_spectra/aspirin.rs
FEATURE_ID=2
PEPMASS=181.049
CHARGE=1
RTINSECONDS=20.0
MSLEVEL=2
SCANS=2
50.0149 49377.4
51.0228 53422.1
53.0385 454244.5
55.0177 57881.0
65.0384 1997532.0
77.0383 825848.2
79.054 1153465.5
80.0254 96202.8
81.0334 58626.8
91.0541 44573.8
92.0254 394779.8
93.0333 1129287.8
94.0411 56357.2
95.049 1654496.5
98.0361 72487.3
105.0333 707899.4
105.0445 1119356.4
107.0489 207437.4
111.0439 587441.8
120.0203 166384.0
121.0282 9695889.0
121.0394 2506571.2
133.0282 5824675.0
135.0438 6124332.0
138.0308 78621.8
149.0231 34285450.0
163.0386 18191732.0
167.0337 69049.3
181.0491 120675.9
END IONS

BEGIN IONS
TITLE=Cocaine reference spectrum
SOURCE=mass-spectrometry-traits/src/traits/reference_spectra/cocaine.rs
FEATURE_ID=3
PEPMASS=304.15314
CHARGE=1
RTINSECONDS=30.0
MSLEVEL=2
SCANS=3
82.06479 13342.493
105.03325 3264.1335
109.213745 1584.2748
119.04921 2382.931
150.0914 3257.3662
182.11768 133504.3
185.80469 1849.1401
226.57907 1391.7345
304.15314 86052.375
END IONS

BEGIN IONS
TITLE=Glucose reference spectrum
SOURCE=mass-spectrometry-traits/src/traits/reference_spectra/glucose.rs
FEATURE_ID=4
PEPMASS=203.05
CHARGE=1
RTINSECONDS=40.0
MSLEVEL=2
SCANS=4
82.95215 798.8589
105.27045 1257.2534
112.7894 3923.249
121.208 1952.9655
129.1167 169.58734
131.1041 412.05518
131.99069 309.9395
135.00793 520.56915
142.50119 555.74243
143.1029 13786.814
158.09225 1758.8164
160.23529 408.88977
173.10046 892.3469
185.15268 9220.535
END IONS

BEGIN IONS
TITLE=Phenylalanine reference spectrum
SOURCE=mass-spectrometry-traits/src/traits/reference_spectra/phenylalanine.rs
FEATURE_ID=5
PEPMASS=166.086
CHARGE=1
RTINSECONDS=50.0
MSLEVEL=2
SCANS=5
84.32965 802.63324
95.26145 527.7473
104.25554 232.55643
107.15116 297.47217
108.49341 113.4012
120.35417 8457614.0
121.33274 5223.299
122.17409 1784.4019
123.06145 733.6038
123.87631 514.92334
125.26976 680.6416
127.95097 341.42133
131.03027 44541.625
131.89882 596.8916
132.98749 781.1204
133.96695 2167.02
135.2178 130.05476
136.68353 264.5392
138.05045 1620.136
148.04901 55389.79
149.00742 364218.6
149.63423 4676.8096
150.75449 590.6233
END IONS
";

/// Complete SPLASH computation report for an MGF input document.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SplashReport {
    records: Vec<SplashRecord>,
}

impl SplashReport {
    /// Creates a report from already-computed records.
    #[must_use]
    pub const fn new(records: Vec<SplashRecord>) -> Self {
        Self { records }
    }

    /// Returns every spectrum record in input order.
    #[must_use]
    pub fn records(&self) -> &[SplashRecord] {
        &self.records
    }

    /// Returns the number of spectra found in the input.
    #[must_use]
    pub const fn total_count(&self) -> usize {
        self.records.len()
    }

    /// Returns the number of spectra with a generated SPLASH code.
    #[must_use]
    pub fn success_count(&self) -> usize {
        self.records
            .iter()
            .filter(|record| record.status().is_generated())
            .count()
    }

    /// Returns the number of spectra whose SPLASH computation failed.
    #[must_use]
    pub fn failure_count(&self) -> usize {
        self.records
            .iter()
            .filter(|record| record.status().is_failed())
            .count()
    }

    /// Returns the number of distinct generated SPLASH codes shared by multiple spectra.
    #[must_use]
    pub fn duplicate_splash_count(&self) -> usize {
        self.duplicate_splash_codes().count()
    }

    /// Returns distinct generated SPLASH codes shared by multiple spectra.
    pub fn duplicate_splash_codes(&self) -> impl Iterator<Item = &str> {
        self.generated_splash_counts()
            .into_iter()
            .filter_map(|(code, count)| (count > 1).then_some(code))
    }

    fn generated_splash_counts(&self) -> BTreeMap<&str, usize> {
        let mut counts = BTreeMap::<&str, usize>::new();
        for code in self
            .records
            .iter()
            .filter_map(|record| record.status().code())
        {
            counts
                .entry(code)
                .and_modify(|count| *count += 1)
                .or_insert(1);
        }
        counts
    }

    /// Returns true when the report contains no spectra.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.records.is_empty()
    }

    /// Serializes all report records as tab-separated values.
    #[must_use]
    pub fn to_tsv(&self) -> String {
        let mut output =
            String::from("spectrum\ttitle\tfeature_id\tpepmass\tstatus\tsplash\terror\n");
        for record in &self.records {
            let (status, splash, error) = match record.status() {
                SplashStatus::Generated(code) => ("ok", code.as_str(), ""),
                SplashStatus::Failed(message) => ("error", "", message.as_str()),
            };
            let _ = writeln!(
                &mut output,
                "{}\t{}\t{}\t{}\t{}\t{}\t{}",
                record.index(),
                escape_tsv_field(record.title()),
                escape_tsv_field(record.feature_id().unwrap_or_default()),
                escape_tsv_field(record.pepmass()),
                status,
                escape_tsv_field(splash),
                escape_tsv_field(error),
            );
        }
        output
    }
}

/// Parses MGF text and returns canonical MGF blocks annotated with SPLASH metadata.
///
/// Spectra with a generated SPLASH code receive a `SPLASH=` metadata line.
/// Spectra whose SPLASH computation fails receive a `SPLASH_ERROR=` metadata
/// line instead, so the downloaded MGF keeps per-spectrum failure context.
///
/// # Errors
///
/// Returns an error when the MGF document cannot be parsed.
pub fn mgf_with_splash(input: &str) -> Result<String, MgfSplashError> {
    if input.trim().is_empty() {
        return Ok(String::new());
    }

    let mut output = String::new();
    for (offset, spectrum) in MGFIter::<f64, _>::from_document(input).enumerate() {
        let spectrum = spectrum
            .map_err(|error| MgfSplashError::new(format!("MGF parsing failed: {error}")))?;
        if offset > 0 {
            output.push('\n');
        }
        write_mgf_record_with_splash(&mut output, &spectrum, &splash_status_for(&spectrum));
    }

    Ok(output)
}

/// SPLASH computation result for one spectrum.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SplashRecord {
    index: usize,
    title: String,
    feature_id: Option<String>,
    pepmass: String,
    status: SplashStatus,
}

impl SplashRecord {
    /// Creates a spectrum-level SPLASH record.
    #[must_use]
    pub const fn new(
        index: usize,
        title: String,
        feature_id: Option<String>,
        pepmass: String,
        status: SplashStatus,
    ) -> Self {
        Self {
            index,
            title,
            feature_id,
            pepmass,
            status,
        }
    }

    /// Returns the one-based spectrum index in the MGF document.
    #[must_use]
    pub const fn index(&self) -> usize {
        self.index
    }

    /// Returns the spectrum title or a generated fallback title.
    #[must_use]
    pub fn title(&self) -> &str {
        &self.title
    }

    /// Returns the optional MGF feature identifier.
    #[must_use]
    pub fn feature_id(&self) -> Option<&str> {
        self.feature_id.as_deref()
    }

    /// Returns the MGF `PEPMASS` precursor mass-to-charge value.
    #[must_use]
    pub fn pepmass(&self) -> &str {
        &self.pepmass
    }

    /// Returns the SPLASH computation status.
    #[must_use]
    pub const fn status(&self) -> &SplashStatus {
        &self.status
    }
}

/// SPLASH computation status for one spectrum.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "status", content = "value", rename_all = "snake_case")]
pub enum SplashStatus {
    /// SPLASH code generated successfully.
    Generated(String),
    /// Error message produced while generating the SPLASH code.
    Failed(String),
}

impl SplashStatus {
    /// Returns true when this status contains a generated SPLASH code.
    #[must_use]
    pub const fn is_generated(&self) -> bool {
        matches!(self, Self::Generated(_))
    }

    /// Returns true when this status contains an error message.
    #[must_use]
    pub const fn is_failed(&self) -> bool {
        matches!(self, Self::Failed(_))
    }

    /// Returns the generated SPLASH code, if any.
    #[must_use]
    pub fn code(&self) -> Option<&str> {
        match self {
            Self::Generated(code) => Some(code),
            Self::Failed(_) => None,
        }
    }

    /// Returns the failure message, if any.
    #[must_use]
    pub fn message(&self) -> Option<&str> {
        match self {
            Self::Generated(_) => None,
            Self::Failed(message) => Some(message),
        }
    }
}

/// Error returned when MGF parsing fails before a report can be built.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct MgfSplashError {
    message: String,
}

impl MgfSplashError {
    /// Creates a new MGF SPLASH error.
    #[must_use]
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }

    /// Returns the human-readable error message.
    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl Display for MgfSplashError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> FmtResult {
        formatter.write_str(&self.message)
    }
}

impl Error for MgfSplashError {}

/// Parses MGF text and computes a SPLASH report for each spectrum.
///
/// # Errors
///
/// Returns an error when the MGF document cannot be parsed.
pub fn splash_report_from_mgf(input: &str) -> Result<SplashReport, MgfSplashError> {
    if input.trim().is_empty() {
        return Ok(SplashReport::new(Vec::new()));
    }

    let mut records = Vec::new();
    for (offset, spectrum) in MGFIter::<f64, _>::from_document(input).enumerate() {
        let spectrum = spectrum
            .map_err(|error| MgfSplashError::new(format!("MGF parsing failed: {error}")))?;
        let status = splash_status_for(&spectrum);
        records.push(SplashRecord::new(
            offset + 1,
            spectrum_title(&spectrum, offset + 1),
            spectrum.feature_id().map(ToOwned::to_owned),
            spectrum.precursor_mz().to_string(),
            status,
        ));
    }

    Ok(SplashReport::new(records))
}

fn splash_status_for(spectrum: &MascotGenericFormat<f64>) -> SplashStatus {
    match splash_with_latest_traits(spectrum) {
        Ok(code) => SplashStatus::Generated(code),
        Err(error) => SplashStatus::Failed(error),
    }
}

fn splash_with_latest_traits(spectrum: &MascotGenericFormat<f64>) -> Result<String, String> {
    let mut converted = GenericSpectrum::try_with_capacity(spectrum.precursor_mz(), spectrum.len())
        .map_err(|error| error.to_string())?;
    for (mz, intensity) in spectrum.peaks() {
        converted
            .add_peak(mz, intensity)
            .map_err(|error| error.to_string())?;
    }
    converted.splash().map_err(|error| error.to_string())
}

fn spectrum_title(spectrum: &MascotGenericFormat<f64>, index: usize) -> String {
    spectrum
        .metadata()
        .arbitrary_metadata_value("TITLE")
        .or_else(|| spectrum.metadata().arbitrary_metadata_value("NAME"))
        .map_or_else(|| format!("Spectrum {index}"), ToOwned::to_owned)
}

fn write_mgf_record_with_splash(
    output: &mut String,
    spectrum: &MascotGenericFormat<f64>,
    status: &SplashStatus,
) {
    push_mgf_text_line(output, "BEGIN IONS");
    let metadata = spectrum.metadata();
    if let Some(feature_id) = spectrum.feature_id() {
        push_mgf_metadata_line(output, "FEATURE_ID", feature_id);
    }
    push_mgf_metadata_line(output, "PEPMASS", spectrum.precursor_mz());
    if let Some(charge) = spectrum.charge() {
        push_mgf_metadata_line(output, "CHARGE", charge);
    }
    if let Some(retention_time) = metadata.retention_time() {
        push_mgf_metadata_line(output, "RTINSECONDS", retention_time);
    }
    push_mgf_metadata_line(output, "MSLEVEL", spectrum.level());
    if let Some(filename) = metadata.filename() {
        push_mgf_metadata_line(output, "FILENAME", filename);
    }
    if let Some(smiles) = metadata.smiles() {
        push_mgf_metadata_line(output, "SMILES", smiles);
    }
    if let Some(formula) = spectrum.formula() {
        push_mgf_metadata_line(output, "FORMULA", formula);
    }
    match status {
        SplashStatus::Generated(code) => push_mgf_metadata_line(output, "SPLASH", code),
        SplashStatus::Failed(message) => {
            push_mgf_metadata_line(output, "SPLASH_ERROR", escape_mgf_metadata_value(message));
        }
    }
    if let Some(ion_mode) = spectrum.ion_mode() {
        push_mgf_metadata_line(output, "IONMODE", ion_mode);
    }
    if let Some(source_instrument) = spectrum.source_instrument() {
        push_mgf_metadata_line(output, "SOURCE_INSTRUMENT", source_instrument);
    }
    for (key, value) in metadata.arbitrary_metadata() {
        push_mgf_metadata_line(output, key, value);
    }
    if let Some(scans) = spectrum.scans() {
        push_mgf_metadata_line(output, "SCANS", scans);
    }
    for (mz, intensity) in spectrum.peaks() {
        let _ = writeln!(output, "{mz} {intensity}");
    }
    push_mgf_text_line(output, "END IONS");
}

fn push_mgf_text_line(output: &mut String, line: &str) {
    let _ = writeln!(output, "{line}");
}

fn push_mgf_metadata_line(output: &mut String, key: impl Display, value: impl Display) {
    let _ = writeln!(output, "{key}={value}");
}

fn escape_mgf_metadata_value(value: &str) -> String {
    value
        .chars()
        .map(|character| match character {
            '\n' | '\r' => ' ',
            _ => character,
        })
        .collect()
}

/// Request sent from the UI thread to the SPLASH web worker.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum MgfWorkerRequest {
    /// Cancels any worker result older than this token.
    Cancel {
        /// Monotonic request token.
        token: u64,
    },
    /// Computes SPLASH records from an MGF input string.
    Process {
        /// Monotonic request token.
        token: u64,
        /// MGF input text.
        input: String,
    },
    /// Builds MGF text with `SPLASH=` metadata added to each spectrum.
    AnnotateMgf {
        /// Monotonic request token.
        token: u64,
        /// MGF input text.
        input: String,
    },
}

/// Response sent from the SPLASH web worker back to the UI thread.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum MgfWorkerResponse {
    /// Worker is initialized and can accept requests.
    Ready,
    /// Worker has started processing a request.
    Progress {
        /// Monotonic request token.
        token: u64,
        /// Progress label for the UI.
        label: String,
    },
    /// Worker completed a request successfully.
    Complete {
        /// Monotonic request token.
        token: u64,
        /// Completed SPLASH report.
        report: SplashReport,
    },
    /// Worker completed an annotated MGF download document.
    AnnotatedMgf {
        /// Monotonic request token.
        token: u64,
        /// MGF text with SPLASH metadata added.
        document: String,
    },
    /// Worker failed while processing a request.
    Fatal {
        /// Monotonic request token.
        token: u64,
        /// Human-readable failure message.
        message: String,
    },
    /// Worker failed while preparing an annotated MGF document.
    AnnotationFatal {
        /// Monotonic request token.
        token: u64,
        /// Human-readable failure message.
        message: String,
    },
}

impl MgfWorkerResponse {
    /// Returns the request token associated with this response.
    #[must_use]
    pub const fn token(&self) -> u64 {
        match self {
            Self::Ready => 0,
            Self::Progress { token, .. }
            | Self::Complete { token, .. }
            | Self::AnnotatedMgf { token, .. }
            | Self::Fatal { token, .. }
            | Self::AnnotationFatal { token, .. } => *token,
        }
    }
}

fn escape_tsv_field(value: &str) -> String {
    value
        .chars()
        .map(|character| match character {
            '\t' | '\n' | '\r' => ' ',
            _ => character,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const TWO_RECORD_MGF: &str = r"BEGIN IONS
FEATURE_ID=7
PEPMASS=250.0
CHARGE=1
RTINSECONDS=10.0
MSLEVEL=2
100.0 10.0
200.0 20.0
SCANS=7
END IONS

BEGIN IONS
FEATURE_ID=8
PEPMASS=350.0
CHARGE=1
RTINSECONDS=12.0
MSLEVEL=2
150.0 15.0
250.0 25.0
SCANS=8
END IONS
";

    #[test]
    fn parses_mgf_records_and_generates_splash_codes() -> Result<(), MgfSplashError> {
        let report = splash_report_from_mgf(TWO_RECORD_MGF)?;

        assert_eq!(report.total_count(), 2);
        assert_eq!(report.success_count(), 2);
        assert_eq!(report.failure_count(), 0);

        let [first, second] = report.records() else {
            return Err(MgfSplashError::new("expected exactly two records"));
        };
        assert_eq!(first.index(), 1);
        assert_eq!(first.title(), "Spectrum 1");
        assert_eq!(first.feature_id(), Some("7"));
        assert_eq!(first.pepmass(), "250");
        assert_eq!(
            first.status().code(),
            Some("splash10-0udi-0490000000-4425acda10ed7d4709bd")
        );

        assert_eq!(second.title(), "Spectrum 2");
        assert_eq!(second.feature_id(), Some("8"));
        assert_eq!(second.pepmass(), "350");
        assert!(
            second
                .status()
                .code()
                .is_some_and(|code| code.starts_with("splash10-"))
        );
        Ok(())
    }

    #[test]
    fn sample_mgf_contains_named_reference_spectra_with_distinct_codes()
    -> Result<(), MgfSplashError> {
        let report = splash_report_from_mgf(SAMPLE_MGF)?;

        assert_eq!(report.total_count(), 5);
        assert_eq!(report.success_count(), 5);
        assert_eq!(report.failure_count(), 0);
        assert_eq!(report.duplicate_splash_count(), 0);

        let titles: Vec<&str> = report.records().iter().map(SplashRecord::title).collect();
        assert_eq!(
            titles,
            [
                "Two peak SPLASH sanity check",
                "Aspirin reference spectrum",
                "Cocaine reference spectrum",
                "Glucose reference spectrum",
                "Phenylalanine reference spectrum",
            ]
        );

        let codes: Result<Vec<&str>, MgfSplashError> = report
            .records()
            .iter()
            .map(|record| {
                record
                    .status()
                    .code()
                    .ok_or_else(|| MgfSplashError::new("expected generated SPLASH code"))
            })
            .collect();
        let codes = codes?;
        assert_eq!(
            codes.first().copied(),
            Some("splash10-0udi-0490000000-4425acda10ed7d4709bd")
        );

        for (left_index, left_code) in codes.iter().enumerate() {
            for right_code in codes.iter().skip(left_index + 1) {
                assert_ne!(left_code, right_code);
            }
        }
        Ok(())
    }

    #[test]
    fn duplicate_splash_count_tracks_distinct_repeated_codes() {
        let report = SplashReport::new(vec![
            SplashRecord::new(
                1,
                String::from("First aspirin replicate"),
                None,
                String::from("181.049"),
                SplashStatus::Generated(String::from("splash10-a")),
            ),
            SplashRecord::new(
                2,
                String::from("Second aspirin replicate"),
                None,
                String::from("181.049"),
                SplashStatus::Generated(String::from("splash10-a")),
            ),
            SplashRecord::new(
                3,
                String::from("First cocaine replicate"),
                None,
                String::from("304.15314"),
                SplashStatus::Generated(String::from("splash10-b")),
            ),
            SplashRecord::new(
                4,
                String::from("Second cocaine replicate"),
                None,
                String::from("304.15314"),
                SplashStatus::Generated(String::from("splash10-b")),
            ),
            SplashRecord::new(
                5,
                String::from("Third cocaine replicate"),
                None,
                String::from("304.15314"),
                SplashStatus::Generated(String::from("splash10-b")),
            ),
            SplashRecord::new(
                6,
                String::from("Unique glucose spectrum"),
                None,
                String::from("203.05"),
                SplashStatus::Generated(String::from("splash10-c")),
            ),
            SplashRecord::new(
                7,
                String::from("Failed spectrum"),
                None,
                String::from("166.086"),
                SplashStatus::Failed(String::from("all intensities are zero")),
            ),
        ]);

        assert_eq!(report.duplicate_splash_count(), 2);
        assert_eq!(
            report.duplicate_splash_codes().collect::<Vec<_>>(),
            ["splash10-a", "splash10-b"]
        );
    }

    #[test]
    fn empty_input_returns_empty_report() -> Result<(), MgfSplashError> {
        let report = splash_report_from_mgf(" \n\t ")?;

        assert!(report.is_empty());
        assert_eq!(
            report.to_tsv(),
            "spectrum\ttitle\tfeature_id\tpepmass\tstatus\tsplash\terror\n"
        );
        Ok(())
    }

    #[test]
    fn invalid_mgf_is_reported_as_parse_error() -> Result<(), MgfSplashError> {
        let Err(error) = splash_report_from_mgf("BEGIN IONS\nPEPMASS=not-a-number\nEND IONS")
        else {
            return Err(MgfSplashError::new("expected invalid MGF to fail"));
        };

        let error = error.to_string();
        assert!(error.starts_with("MGF parsing failed:"));
        assert!(error.contains("line 2"));
        assert!(error.contains("PEPMASS=not-a-number"));
        assert!(error.contains("could not parse precursor m/z"));
        Ok(())
    }

    #[test]
    fn mgf_download_output_adds_splash_metadata() -> Result<(), MgfSplashError> {
        let annotated = mgf_with_splash(TWO_RECORD_MGF)?;

        assert_eq!(annotated.matches("BEGIN IONS").count(), 2);
        assert_eq!(annotated.matches("SPLASH=").count(), 2);
        assert!(annotated.contains("SPLASH=splash10-0udi-0490000000-4425acda10ed7d4709bd"));
        assert!(!annotated.contains("SPLASH_ERROR="));

        let reparsed = splash_report_from_mgf(&annotated)?;
        assert_eq!(reparsed.total_count(), 2);
        assert_eq!(reparsed.success_count(), 2);
        Ok(())
    }

    #[test]
    fn mgf_download_output_keeps_splash_failures_visible() -> Result<(), MgfSplashError> {
        let mut spectra = MGFIter::<f64, _>::from_document(TWO_RECORD_MGF);
        let Some(Ok(spectrum)) = spectra.next() else {
            return Err(MgfSplashError::new("expected one parsed spectrum"));
        };
        let mut annotated = String::new();
        write_mgf_record_with_splash(
            &mut annotated,
            &spectrum,
            &SplashStatus::Failed(String::from("all intensities are zero")),
        );

        assert!(annotated.contains("SPLASH_ERROR="));
        assert!(annotated.contains("all intensities are zero"));
        assert!(!annotated.contains("SPLASH=splash"));
        Ok(())
    }

    #[test]
    fn tsv_output_contains_successes_and_failures() {
        let report = SplashReport::new(vec![
            SplashRecord::new(
                1,
                String::from("Two peak SPLASH sanity check"),
                Some(String::from("7")),
                String::from("250"),
                SplashStatus::Generated(String::from(
                    "splash10-0udi-0490000000-4425acda10ed7d4709bd",
                )),
            ),
            SplashRecord::new(
                2,
                String::from("Empty spectrum"),
                Some(String::from("8")),
                String::from("350"),
                SplashStatus::Failed(String::from("all intensities are zero")),
            ),
        ]);
        let tsv = report.to_tsv();

        assert!(
            tsv.contains("1\tTwo peak SPLASH sanity check\t7\t250\tok\tsplash10-0udi-0490000000-4425acda10ed7d4709bd\t")
        );
        assert!(tsv.contains("2\tEmpty spectrum\t8\t350\terror\t"));
    }

    #[test]
    fn tsv_output_includes_all_records() {
        let records = (1..=55)
            .map(|index| {
                SplashRecord::new(
                    index,
                    format!("Spectrum {index}"),
                    Some(format!("{index}")),
                    format!("{}.5", 100_usize + index),
                    SplashStatus::Generated(format!("splash10-test-{index:010}")),
                )
            })
            .collect();
        let report = SplashReport::new(records);
        let tsv = report.to_tsv();

        assert_eq!(tsv.lines().count(), 56);
        assert!(tsv.contains("55\tSpectrum 55\t55\t155.5\tok\tsplash10-test-0000000055\t"));
    }
}
