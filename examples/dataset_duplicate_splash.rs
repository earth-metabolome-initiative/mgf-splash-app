//! Counts duplicated `(PEPMASS, SPLASH)` entries in downloadable MGF datasets.

use std::{
    collections::BTreeMap,
    error::Error,
    io::{self, Write as _},
    time::Duration,
};

use mascot_rs::prelude::{
    Dataset, GenericSpectrum, MGFVec, MascotGenericFormat, Spectrum, SpectrumMut, SpectrumSplash,
};
use tokio::time::sleep;

type ExperimentResult<T> = Result<T, Box<dyn Error + Send + Sync>>;

const DATASET_LOAD_ATTEMPTS: usize = 64;
const DATASET_LOAD_RETRY_DELAY: Duration = Duration::from_secs(30);

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct SpectrumEntry {
    pepmass: String,
    splash: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct SplashPepmassState {
    first_pepmass: String,
    multiple_pepmass: bool,
}

impl SplashPepmassState {
    const fn new(pepmass: String) -> Self {
        Self {
            first_pepmass: pepmass,
            multiple_pepmass: false,
        }
    }

    fn observe(&mut self, pepmass: &str) {
        if self.first_pepmass != pepmass {
            self.multiple_pepmass = true;
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct DatasetReport {
    name: &'static str,
    spectra: usize,
    hashed_spectra: usize,
    unique_entries: usize,
    duplicate_entries: usize,
    duplicate_spectra: usize,
    splashes_with_multiple_pepmass: usize,
    splash_failures: usize,
}

#[tokio::main]
async fn main() -> ExperimentResult<()> {
    let reports = [
        analyze_dataset("MassSpecGym", MGFVec::<f64>::mass_spec_gym().verbose()).await?,
        analyze_dataset("GNPS", MGFVec::<f64>::gnps().verbose()).await?,
    ];

    write_reports(&reports)?;
    Ok(())
}

async fn analyze_dataset<D, L>(name: &'static str, dataset: D) -> ExperimentResult<DatasetReport>
where
    D: Clone + Dataset<Load = L>,
    L: AsRef<MGFVec<f64>>,
{
    let load = load_dataset(name, dataset).await?;
    Ok(analyze_spectra(name, load.as_ref()))
}

async fn load_dataset<D, L>(name: &str, dataset: D) -> ExperimentResult<L>
where
    D: Clone + Dataset<Load = L>,
{
    let mut attempt = 1_usize;
    loop {
        match dataset.clone().load().await {
            Ok(load) => return Ok(load),
            Err(error) if attempt == DATASET_LOAD_ATTEMPTS => return Err(Box::new(error)),
            Err(error) => {
                write_retry_notice(name, attempt, &error)?;
                sleep(DATASET_LOAD_RETRY_DELAY).await;
                attempt += 1;
            }
        }
    }
}

fn write_retry_notice(name: &str, attempt: usize, error: &dyn Error) -> io::Result<()> {
    let mut stderr = io::stderr().lock();
    writeln!(
        stderr,
        "Retrying {name} after attempt {attempt}/{DATASET_LOAD_ATTEMPTS} failed: {error}"
    )
}

fn analyze_spectra(name: &'static str, spectra: &MGFVec<f64>) -> DatasetReport {
    let mut entries = BTreeMap::<SpectrumEntry, usize>::new();
    let mut pepmass_state_by_splash = BTreeMap::<String, SplashPepmassState>::new();
    let mut splash_failures = 0_usize;

    for spectrum in spectra {
        let Ok(splash) = splash_for(spectrum) else {
            splash_failures += 1;
            continue;
        };
        let pepmass = spectrum.precursor_mz().to_string();
        let entry = SpectrumEntry {
            pepmass: pepmass.clone(),
            splash: splash.clone(),
        };
        pepmass_state_by_splash
            .entry(splash)
            .and_modify(|state| state.observe(&pepmass))
            .or_insert_with(|| SplashPepmassState::new(pepmass));
        entries
            .entry(entry)
            .and_modify(|count| *count += 1)
            .or_insert(1);
    }

    let duplicate_entries = entries.values().filter(|&&count| count > 1).count();
    let duplicate_spectra = entries.values().map(|&count| count.saturating_sub(1)).sum();
    let splashes_with_multiple_pepmass = pepmass_state_by_splash
        .values()
        .filter(|state| state.multiple_pepmass)
        .count();

    DatasetReport {
        name,
        spectra: spectra.len(),
        hashed_spectra: entries.values().sum(),
        unique_entries: entries.len(),
        duplicate_entries,
        duplicate_spectra,
        splashes_with_multiple_pepmass,
        splash_failures,
    }
}

fn splash_for(spectrum: &MascotGenericFormat<f64>) -> ExperimentResult<String> {
    let mut generic = GenericSpectrum::try_with_capacity(spectrum.precursor_mz(), spectrum.len())?;
    for (mz, intensity) in spectrum.peaks() {
        generic.add_peak(mz, intensity)?;
    }
    Ok(generic.splash()?)
}

fn write_reports(reports: &[DatasetReport]) -> ExperimentResult<()> {
    let mut stdout = io::stdout().lock();
    writeln!(
        stdout,
        "dataset\tspectra\thashed_spectra\tunique_pepmass_splash\tduplicate_entries\tduplicate_spectra\tsplashes_with_multiple_pepmass\tsplash_failures"
    )?;
    for report in reports {
        writeln!(
            stdout,
            "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
            report.name,
            report.spectra,
            report.hashed_spectra,
            report.unique_entries,
            report.duplicate_entries,
            report.duplicate_spectra,
            report.splashes_with_multiple_pepmass,
            report.splash_failures
        )?;
    }
    Ok(())
}
