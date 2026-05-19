//! Dioxus browser UI for computing SPLASH identifiers from MGF text.

use std::{
    cell::{Cell, RefCell},
    collections::BTreeMap,
    rc::Rc,
};

use dioxus::html::{FileData, HasFileData};
use dioxus::prelude::*;
use dioxus_free_icons::icons::{
    fa_brands_icons::FaGithub,
    ld_icons::{
        LdCircleAlert, LdCircleCheck, LdDownload, LdFileText, LdFingerprint, LdHash, LdSparkles,
        LdTable2, LdUpload,
    },
};
use dioxus_free_icons::{Icon, IconShape};

use mgf_splash_app::{MgfWorkerRequest, SAMPLE_MGF, SplashRecord, SplashReport, SplashStatus};

#[cfg(target_arch = "wasm32")]
use js_sys::Array;
#[cfg(target_arch = "wasm32")]
use mgf_splash_app::MgfWorkerResponse;
#[cfg(not(target_arch = "wasm32"))]
use mgf_splash_app::{mgf_with_splash, splash_report_from_mgf};

#[cfg(target_arch = "wasm32")]
use wasm_bindgen::{JsCast, JsValue, closure::Closure};
#[cfg(target_arch = "wasm32")]
use web_sys::{
    Blob, ErrorEvent, HtmlAnchorElement, MessageEvent, Url, Worker, WorkerOptions, WorkerType,
};

#[cfg(target_arch = "wasm32")]
const WORKER_SCRIPT: &str = "generated/mgf-splash-worker.js";
#[cfg(target_arch = "wasm32")]
const LOADING_DELAY_MS: i32 = 700;
const TABLE_PREVIEW_LIMIT: usize = 50;
const DUPLICATE_HUE_OFFSET: usize = 43;
const DUPLICATE_HUE_STEP: usize = 137;
const DUPLICATE_HUE_RANGE: usize = 360;

fn main() {
    dioxus::launch(App);
}

#[derive(Clone, PartialEq)]
#[cfg_attr(
    not(target_arch = "wasm32"),
    expect(dead_code, reason = "native builds keep browser-only state variants")
)]
enum ReportState {
    Empty,
    Loading { label: String },
    Ready(SplashReport),
    Fatal(String),
}

#[derive(Clone)]
struct LoadingControls {
    request_inflight: Rc<Cell<Option<u64>>>,
    loading_visible: Rc<Cell<bool>>,
    loading_timeout_id: Rc<Cell<Option<i32>>>,
    pending_loading_label: Rc<RefCell<Option<String>>>,
}

impl LoadingControls {
    fn reset(&self) {
        self.request_inflight.set(None);
        self.loading_visible.set(false);
        self.pending_loading_label.borrow_mut().take();
        clear_loading_timeout(&self.loading_timeout_id);
    }
}

#[derive(Clone)]
struct SplashRuntime {
    report_state: Signal<ReportState>,
    worker_client: Result<Rc<SplashWorker>, String>,
    request_token: Rc<Cell<u64>>,
    loading: LoadingControls,
}

impl SplashRuntime {
    fn process(&self, input: &str) {
        let token = next_request_token(&self.request_token);
        self.loading.request_inflight.set(Some(token));
        self.loading.loading_visible.set(false);
        self.loading.pending_loading_label.borrow_mut().take();
        clear_loading_timeout(&self.loading.loading_timeout_id);

        let mut report_state = self.report_state;
        if input.trim().is_empty() {
            self.loading.reset();
            report_state.set(ReportState::Empty);
            let _ignored =
                send_worker_request(&self.worker_client, &MgfWorkerRequest::Cancel { token });
            return;
        }

        schedule_loading_timeout(report_state, &self.loading, token);
        let request = MgfWorkerRequest::Process {
            token,
            input: input.to_owned(),
        };
        if let Err(message) = send_worker_request(&self.worker_client, &request) {
            self.loading.reset();
            report_state.set(ReportState::Fatal(message));
        }
    }

    fn download_mgf(&self, input: &str, mut download_status: Signal<String>) {
        let token = next_request_token(&self.request_token);
        download_status.set(String::from("Preparing MGF download."));
        let request = MgfWorkerRequest::AnnotateMgf {
            token,
            input: input.to_owned(),
        };
        if let Err(message) = send_worker_request(&self.worker_client, &request) {
            download_status.set(message);
        }
    }
}

#[component]
fn App() -> Element {
    let mut input = use_signal(String::new);
    let mut file_status = use_signal(String::new);
    let mut download_status = use_signal(String::new);
    let report_state = use_signal(|| ReportState::Empty);
    let request_token = use_hook(|| Rc::new(Cell::new(0_u64)));
    let request_inflight = use_hook(|| Rc::new(Cell::new(None::<u64>)));
    let loading_visible = use_hook(|| Rc::new(Cell::new(false)));
    let loading_timeout_id = use_hook(|| Rc::new(Cell::new(None::<i32>)));
    let pending_loading_label = use_hook(|| Rc::new(RefCell::new(None::<String>)));
    let loading = LoadingControls {
        request_inflight,
        loading_visible,
        loading_timeout_id,
        pending_loading_label,
    };
    let worker_client = use_hook({
        let loading = loading.clone();
        let request_token = request_token.clone();
        move || create_worker_client(report_state, request_token, loading, download_status)
    });
    let runtime = Rc::new(SplashRuntime {
        report_state,
        worker_client,
        request_token,
        loading,
    });

    let input_value = input();
    let processing_runtime = runtime.clone();
    use_effect(move || {
        let next_input = input();
        download_status.set(String::new());
        processing_runtime.process(&next_input);
    });

    let state = report_state();
    let tsv = match &state {
        ReportState::Ready(report) => report.to_tsv(),
        ReportState::Empty | ReportState::Loading { .. } | ReportState::Fatal(_) => String::new(),
    };
    let can_download = matches!(&state, ReportState::Ready(report) if !report.records().is_empty());
    let tsv_for_download = tsv;
    let input_for_mgf_download = input_value.clone();

    rsx! {
        main { class: "page",
            header { class: "hero",
                div { class: "hero-main",
                    p { class: "eyebrow", "earth metabolome initiative" }
                    h1 {
                        "MGF "
                        span { class: "hero-rust-suffix", "SPLASH" }
                    }
                    p { class: "hero-copy",
                        "Compute SPLASH from Mascot Generic Format spectra directly in the browser."
                    }
                }
                nav { class: "hero-links", aria_label: "Project links",
                    a {
                        class: "hero-link",
                        href: "https://github.com/earth-metabolome-initiative/mgf-splash-app",
                        target: "_blank",
                        rel: "noopener noreferrer",
                        aria_label: "Open MGF SPLASH App source code on GitHub",
                        title: "Open MGF SPLASH App source code on GitHub",
                        {app_icon(FaGithub, "GitHub repository")}
                        "Source code"
                    }
                    a {
                        class: "hero-link",
                        href: "https://github.com/LucaCappelletti94/mascot-rs",
                        target: "_blank",
                        rel: "noopener noreferrer",
                        aria_label: "Open mascot-rs GitHub repository",
                        title: "Open mascot-rs GitHub repository",
                        {app_icon(FaGithub, "GitHub repository")}
                        "mascot-rs"
                    }
                    a {
                        class: "hero-link",
                        href: "https://github.com/earth-metabolome-initiative/mass-spectrometry-traits",
                        target: "_blank",
                        rel: "noopener noreferrer",
                        aria_label: "Open mass-spectrometry-traits GitHub repository",
                        title: "Open mass-spectrometry-traits GitHub repository",
                        {app_icon(FaGithub, "GitHub repository")}
                        "SPLASH"
                    }
                    a {
                        class: "hero-link",
                        href: "https://doi.org/10.1038/nbt.3689",
                        target: "_blank",
                        rel: "noopener noreferrer",
                        aria_label: "Open SPLASH paper DOI",
                        title: "Open SPLASH paper DOI",
                        {app_icon(LdFingerprint, "DOI")}
                        "DOI"
                    }
                }
            }

            {splash_definition()}

            section { class: "layout",
                section {
                    class: "panel input-panel",
                    ondragover: move |event| {
                        event.prevent_default();
                    },
                    ondrop: move |event| {
                        event.prevent_default();
                        let files = event.files();
                        if files.is_empty() {
                            if let Some(text) = event.data_transfer().get_as_text() {
                                input.set(text);
                                file_status.set(String::from("Loaded dropped text."));
                            }
                        } else {
                            load_files(files, input, file_status);
                        }
                        download_status.set(String::new());
                    },
                    div { class: "panel-head",
                        div { class: "panel-head-row",
                            div {
                                div { class: "title-with-icon",
                                    {app_icon(LdUpload, "Input")}
                                    h2 { "MGF input" }
                                }
                                p { class: "panel-copy",
                                    "Paste spectra, drop files, or choose one or more MGF files."
                                }
                            }
                            div { class: "input-actions",
                                label {
                                    class: "button button-secondary file-button",
                                    aria_label: "Choose MGF files",
                                    title: "Choose MGF files",
                                    {app_icon(LdUpload, "Choose files")}
                                    "Choose files"
                                    input {
                                        class: "file-input",
                                        r#type: "file",
                                        accept: ".mgf,.txt,text/plain",
                                        multiple: true,
                                        onchange: move |event| {
                                            load_files(event.files(), input, file_status);
                                            download_status.set(String::new());
                                        },
                                    }
                                }
                                button {
                                    class: "button button-secondary",
                                    aria_label: "Load example MGF spectra",
                                    title: "Load example MGF spectra",
                                    onclick: move |_| {
                                        input.set(String::from(SAMPLE_MGF));
                                        file_status.set(String::from(
                                            "Loaded six example spectra, including one deliberate SPLASH duplicate.",
                                        ));
                                        download_status.set(String::new());
                                    },
                                    {app_icon(LdFileText, "Example")}
                                    "Example"
                                }
                            }
                        }
                    }
                    textarea {
                        class: "mgf-input",
                        aria_label: "MGF input",
                        title: "Paste or drop MGF spectra",
                        spellcheck: "false",
                        value: "{input_value}",
                        placeholder: "BEGIN IONS\nPEPMASS=500.0\nCHARGE=1\nMSLEVEL=2\n100.0 20.0\nEND IONS",
                        oninput: move |event| {
                            input.set(event.value());
                            file_status.set(String::new());
                            download_status.set(String::new());
                        },
                    }
                    if !file_status().is_empty() {
                        p { class: "status-note", "{file_status()}" }
                    }
                }

                section { class: "panel result-panel",
                    div { class: "result-head",
                        div { class: "result-head-copy",
                            div { class: "title-with-icon",
                                {app_icon(LdTable2, "Results")}
                                h2 { "SPLASH results" }
                            }
                            {result_summary(&state)}
                        }
                        div { class: "download-actions",
                            button {
                                class: "button button-primary button-download-tsv",
                                aria_label: "Download TSV results",
                                title: "Download TSV results",
                                disabled: !can_download,
                                onclick: move |_| {
                                    match download_tsv(&tsv_for_download) {
                                        Ok(()) => download_status.set(String::from("Downloaded TSV.")),
                                        Err(message) => download_status.set(message),
                                    }
                                },
                                {app_icon(LdDownload, "Download")}
                                "TSV"
                            }
                            button {
                                class: "button button-primary button-download-mgf",
                                aria_label: "Download MGF with SPLASH metadata",
                                title: "Download MGF with SPLASH metadata",
                                disabled: !can_download,
                                onclick: move |_| {
                                    runtime.download_mgf(
                                        &input_for_mgf_download,
                                        download_status,
                                    );
                                },
                                {app_icon(LdDownload, "Download")}
                                "MGF"
                            }
                        }
                    }

                    {result_body(&state)}

                    if !download_status().is_empty() {
                        p { class: "status-note download-note", "{download_status()}" }
                    }
                }
            }
        }
    }
}

fn splash_definition() -> Element {
    rsx! {
        section { class: "splash-definition", aria_label: "SPLASH definition",
            div { class: "title-with-icon splash-definition-title",
                {app_icon(LdHash, "SPLASH")}
                h2 { "What is a SPLASH?" }
            }
            p {
                "A SPLASH is a database-independent identifier for a mass spectrum. It has four dash-separated blocks: version and spectrum type, a prominent-ion prefilter, a coarse similarity histogram, and a truncated SHA-256 hash of the canonicalized peak list."
            }
            p {
                "This app hashes only fragment peak m/z and intensity values. Titles, feature ids, file names, scans, retention times, charges, and PEPMASS/precursor m/z are displayed as context and do not change the SPLASH."
            }
        }
    }
}

fn result_summary(state: &ReportState) -> Element {
    match state {
        ReportState::Empty => rsx! {
            p { class: "panel-copy", "No spectra loaded." }
        },
        ReportState::Loading { label } => rsx! {
            p { class: "panel-copy", "{label}" }
        },
        ReportState::Ready(report) => {
            let duplicate_summary = duplicate_summary_text(report.duplicate_splash_count());
            rsx! {
                div { class: "summary-pills",
                    span { class: "meta-pill",
                        {app_icon(LdHash, "Spectra")}
                        "{report.total_count()} spectra"
                    }
                    span { class: "meta-pill success-pill",
                        {app_icon(LdCircleCheck, "Generated")}
                        "{report.success_count()} generated"
                    }
                    span {
                        class: "meta-pill duplicate-pill",
                        aria_label: "Distinct duplicated SPLASH values",
                        title: "Distinct generated SPLASH values that appear in more than one spectrum",
                        {app_icon(LdHash, "Duplicated SPLASH")}
                        "{duplicate_summary}"
                    }
                    if report.failure_count() > 0 {
                        span { class: "meta-pill error-pill",
                            {app_icon(LdCircleAlert, "Failed")}
                            "{report.failure_count()} failed"
                        }
                    }
                }
            }
        }
        ReportState::Fatal(_) => rsx! {
            p { class: "panel-copy error-copy", "Parsing failed." }
        },
    }
}

fn result_body(state: &ReportState) -> Element {
    match state {
        ReportState::Loading { .. } => rsx! {
            div { class: "empty-state processing-state",
                div { class: "state-icon", {app_icon(LdSparkles, "Processing")} }
                p { class: "empty-title", "Processing MGF spectra" }
                div {
                    class: "progress-shell",
                    role: "progressbar",
                    aria_label: "Processing spectra",
                    aria_valuetext: "Processing",
                    div { class: "progress-bar" }
                }
            }
        },
        ReportState::Empty => rsx! {
            div { class: "empty-state waiting-state",
                div { class: "state-icon", {app_icon(LdUpload, "Waiting for input")} }
                p { class: "empty-title", "Waiting for MGF spectra" }
            }
        },
        ReportState::Ready(report) => {
            let duplicate_styles = duplicate_splash_styles(report);
            rsx! {
                if report.total_count() > TABLE_PREVIEW_LIMIT {
                    p { class: "status-note preview-note",
                        "Showing the first {TABLE_PREVIEW_LIMIT} spectra. Download TSV for all {report.total_count()} results."
                    }
                }
                div { class: "table-wrap",
                    table { class: "result-table",
                        thead {
                            tr {
                                th { "#" }
                                th { "Spectrum" }
                                th { "Feature" }
                                th { "PEPMASS" }
                                th { "SPLASH" }
                            }
                        }
                        tbody {
                            for record in report.records().iter().take(TABLE_PREVIEW_LIMIT) {
                                {result_record_row(
                                    record,
                                    duplicate_splash_style(record, &duplicate_styles),
                                )}
                            }
                        }
                    }
                }
            }
        }
        ReportState::Fatal(error) => rsx! {
            div { class: "empty-state error-state",
                div { class: "state-icon error-icon", {app_icon(LdCircleAlert, "Parse error")} }
                p { class: "empty-title", "MGF parse error" }
                p { class: "error-detail", "{error}" }
            }
        },
    }
}

fn result_record_row(record: &SplashRecord, duplicate_style: Option<&str>) -> Element {
    let is_duplicate = duplicate_style.is_some();
    let row_class = if is_duplicate {
        "duplicate-splash-row"
    } else {
        ""
    };
    let row_style = duplicate_style.unwrap_or_default();
    let row_title = if is_duplicate {
        "This SPLASH is shared by more than one spectrum."
    } else {
        ""
    };

    rsx! {
        tr {
            key: "{record.index()}",
            class: "{row_class}",
            style: "{row_style}",
            title: "{row_title}",
            td { class: "mono-cell", "{record.index()}" }
            td { class: "title-cell", title: "{record.title()}", "{record.title()}" }
            td {
                class: "mono-cell",
                title: "{format_optional_str(record.feature_id())}",
                "{format_optional_str(record.feature_id())}"
            }
            td { class: "mono-cell", title: "{record.pepmass()}", "{record.pepmass()}" }
            td {
                match record.status() {
                    SplashStatus::Generated(code) => rsx! {
                        div { class: "splash-code-wrap",
                            code { class: "splash-code", "{code}" }
                            if is_duplicate {
                                span {
                                    class: "duplicate-splash-label",
                                    aria_label: "Duplicated SPLASH",
                                    title: "This SPLASH is shared by more than one spectrum",
                                    "duplicate"
                                }
                            }
                        }
                    },
                    SplashStatus::Failed(message) => rsx! {
                        span { class: "error-text", "{message}" }
                    },
                }
            }
        }
    }
}

fn duplicate_splash_styles(report: &SplashReport) -> BTreeMap<&str, String> {
    report
        .duplicate_splash_codes()
        .enumerate()
        .map(|(index, code)| (code, duplicate_splash_row_style(index)))
        .collect()
}

fn duplicate_splash_style<'a>(
    record: &SplashRecord,
    duplicate_styles: &'a BTreeMap<&str, String>,
) -> Option<&'a str> {
    record
        .status()
        .code()
        .and_then(|code| duplicate_styles.get(code).map(String::as_str))
}

fn duplicate_splash_row_style(index: usize) -> String {
    let hue =
        (DUPLICATE_HUE_OFFSET + index.saturating_mul(DUPLICATE_HUE_STEP)) % DUPLICATE_HUE_RANGE;
    format!("--duplicate-bg: hsl({hue} 82% 92%); --duplicate-border: hsl({hue} 58% 36%);")
}

fn duplicate_summary_text(count: usize) -> String {
    match count {
        1 => String::from("1 duplicate SPLASH"),
        _ => format!("{count} duplicate SPLASH"),
    }
}

#[cfg(target_arch = "wasm32")]
struct SplashWorker {
    worker: Worker,
    ready: Rc<Cell<bool>>,
    pending_request: Rc<RefCell<Option<MgfWorkerRequest>>>,
    onmessage: Closure<dyn FnMut(MessageEvent)>,
    onerror: Closure<dyn FnMut(ErrorEvent)>,
}

#[cfg(target_arch = "wasm32")]
impl SplashWorker {
    fn new(
        report_state: Signal<ReportState>,
        request_token: Rc<Cell<u64>>,
        loading: LoadingControls,
        download_status: Signal<String>,
    ) -> Result<Self, String> {
        let worker = Self::create_worker()?;
        let ready = Rc::new(Cell::new(false));
        let pending_request = Rc::new(RefCell::new(None::<MgfWorkerRequest>));
        let onmessage = Self::install_onmessage(
            &worker,
            report_state,
            request_token,
            loading.clone(),
            download_status,
            ready.clone(),
            pending_request.clone(),
        );
        let onerror = Self::install_onerror(&worker, report_state, loading);

        Ok(Self {
            worker,
            ready,
            pending_request,
            onmessage,
            onerror,
        })
    }

    fn create_worker() -> Result<Worker, String> {
        let options = WorkerOptions::new();
        options.set_type(WorkerType::Module);
        Worker::new_with_options(WORKER_SCRIPT, &options)
            .map_err(|error| format!("failed to start Web Worker: {}", js_error_text(&error)))
    }

    fn install_onmessage(
        worker: &Worker,
        mut report_state: Signal<ReportState>,
        request_token: Rc<Cell<u64>>,
        loading: LoadingControls,
        mut download_status: Signal<String>,
        ready: Rc<Cell<bool>>,
        pending_request: Rc<RefCell<Option<MgfWorkerRequest>>>,
    ) -> Closure<dyn FnMut(MessageEvent)> {
        let onmessage_worker = worker.clone();
        let onmessage_callback: Box<dyn FnMut(MessageEvent)> = Box::new(move |event| {
            let response = match serde_wasm_bindgen::from_value::<MgfWorkerResponse>(event.data()) {
                Ok(response) => response,
                Err(error) => {
                    loading.reset();
                    report_state.set(ReportState::Fatal(format!(
                        "failed to decode worker response: {error}"
                    )));
                    return;
                }
            };

            if matches!(response, MgfWorkerResponse::Ready) {
                ready.set(true);
                Self::flush_pending_request(&pending_request, &onmessage_worker);
                return;
            }

            if response.token() != request_token.get() {
                return;
            }

            Self::handle_worker_response(
                response,
                &loading,
                &mut report_state,
                &mut download_status,
            );
        });
        let onmessage = Closure::wrap(onmessage_callback);
        worker.set_onmessage(Some(onmessage.as_ref().unchecked_ref()));
        onmessage
    }

    fn install_onerror(
        worker: &Worker,
        mut report_state: Signal<ReportState>,
        loading: LoadingControls,
    ) -> Closure<dyn FnMut(ErrorEvent)> {
        let onerror_callback: Box<dyn FnMut(ErrorEvent)> = Box::new(move |event| {
            loading.reset();
            report_state.set(ReportState::Fatal(format!(
                "SPLASH worker crashed: {}",
                event.message()
            )));
        });
        let onerror = Closure::wrap(onerror_callback);
        worker.set_onerror(Some(onerror.as_ref().unchecked_ref()));
        onerror
    }

    fn flush_pending_request(pending_request: &RefCell<Option<MgfWorkerRequest>>, worker: &Worker) {
        if let Some(request) = pending_request.borrow_mut().take()
            && let Ok(payload) = serde_wasm_bindgen::to_value(&request)
        {
            let _ = worker.post_message(&payload);
        }
    }

    fn handle_worker_response(
        response: MgfWorkerResponse,
        loading: &LoadingControls,
        report_state: &mut Signal<ReportState>,
        download_status: &mut Signal<String>,
    ) {
        match response {
            MgfWorkerResponse::Progress { label, .. } => {
                if loading.loading_visible.get() {
                    clear_loading_timeout(&loading.loading_timeout_id);
                    report_state.set(ReportState::Loading { label });
                } else {
                    loading.pending_loading_label.borrow_mut().replace(label);
                }
            }
            MgfWorkerResponse::Complete { report, .. } => {
                loading.reset();
                report_state.set(ReportState::Ready(report));
            }
            MgfWorkerResponse::Fatal { message, .. } => {
                loading.reset();
                report_state.set(ReportState::Fatal(message));
            }
            MgfWorkerResponse::AnnotatedMgf { document, .. } => match download_mgf(&document) {
                Ok(()) => download_status.set(String::from("Downloaded MGF.")),
                Err(message) => download_status.set(message),
            },
            MgfWorkerResponse::AnnotationFatal { message, .. } => {
                download_status.set(message);
            }
            MgfWorkerResponse::Ready => unreachable!("ready messages return early"),
        }
    }

    fn post(&self, message: &MgfWorkerRequest) -> Result<(), String> {
        if !self.ready.get() {
            self.pending_request.replace(Some(message.clone()));
            return Ok(());
        }

        let payload = serde_wasm_bindgen::to_value(message)
            .map_err(|error| format!("failed to encode worker request: {error}"))?;
        self.worker
            .post_message(&payload)
            .map_err(|error| format!("failed to post worker request: {}", js_error_text(&error)))
    }
}

#[cfg(target_arch = "wasm32")]
impl Drop for SplashWorker {
    fn drop(&mut self) {
        self.worker.set_onmessage(None);
        self.worker.set_onerror(None);
        self.worker.terminate();
        let _ = &self.ready;
        let _ = &self.pending_request;
        let _ = &self.onmessage;
        let _ = &self.onerror;
    }
}

#[cfg(not(target_arch = "wasm32"))]
struct SplashWorker;

#[cfg(not(target_arch = "wasm32"))]
impl SplashWorker {
    fn new(
        _report_state: Signal<ReportState>,
        _request_token: Rc<Cell<u64>>,
        _loading: LoadingControls,
        _download_status: Signal<String>,
    ) -> Result<Self, String> {
        Err(String::from(
            "worker processing is only available in the browser build",
        ))
    }

    #[expect(
        clippy::unused_self,
        reason = "the native stub mirrors the browser worker interface"
    )]
    fn post(&self, message: &MgfWorkerRequest) -> Result<(), String> {
        match message {
            MgfWorkerRequest::Process { input, .. } => splash_report_from_mgf(input)
                .map(|_| ())
                .map_err(|error| error.to_string()),
            MgfWorkerRequest::AnnotateMgf { input, .. } => mgf_with_splash(input)
                .map(|_| ())
                .map_err(|error| error.to_string()),
            MgfWorkerRequest::Cancel { .. } => Ok(()),
        }
    }
}

fn create_worker_client(
    report_state: Signal<ReportState>,
    request_token: Rc<Cell<u64>>,
    loading: LoadingControls,
    download_status: Signal<String>,
) -> Result<Rc<SplashWorker>, String> {
    SplashWorker::new(report_state, request_token, loading, download_status).map(Rc::new)
}

fn next_request_token(request_token: &Cell<u64>) -> u64 {
    let next = request_token.get().wrapping_add(1).max(1);
    request_token.set(next);
    next
}

fn send_worker_request(
    worker_client: &Result<Rc<SplashWorker>, String>,
    request: &MgfWorkerRequest,
) -> Result<(), String> {
    match worker_client {
        Ok(worker_client) => worker_client.post(request),
        Err(message) => Err(message.clone()),
    }
}

#[cfg(target_arch = "wasm32")]
fn clear_loading_timeout(loading_timeout_id: &Cell<Option<i32>>) {
    if let Some(timeout_id) = loading_timeout_id.take()
        && let Some(window) = web_sys::window()
    {
        window.clear_timeout_with_handle(timeout_id);
    }
}

#[cfg(not(target_arch = "wasm32"))]
const fn clear_loading_timeout(_loading_timeout_id: &Cell<Option<i32>>) {}

#[cfg(target_arch = "wasm32")]
fn schedule_loading_timeout(
    mut report_state: Signal<ReportState>,
    loading: &LoadingControls,
    token: u64,
) {
    let callback_loading_timeout_id = loading.loading_timeout_id.clone();
    let callback_loading = loading.clone();
    let callback = Closure::once_into_js(move || {
        callback_loading_timeout_id.set(None);
        if callback_loading.request_inflight.get() == Some(token) {
            callback_loading.loading_visible.set(true);
            let label = callback_loading
                .pending_loading_label
                .borrow_mut()
                .take()
                .unwrap_or_else(|| String::from("Processing MGF spectra"));
            report_state.set(ReportState::Loading { label });
        }
    });

    if let Some(window) = web_sys::window()
        && let Ok(timeout_id) = window.set_timeout_with_callback_and_timeout_and_arguments_0(
            callback.unchecked_ref(),
            LOADING_DELAY_MS,
        )
    {
        loading.loading_timeout_id.set(Some(timeout_id));
    }
}

#[cfg(not(target_arch = "wasm32"))]
const fn schedule_loading_timeout(
    _report_state: Signal<ReportState>,
    _loading: &LoadingControls,
    _token: u64,
) {
}

#[cfg(target_arch = "wasm32")]
fn js_error_text(error: &JsValue) -> String {
    error
        .as_string()
        .filter(|message| !message.is_empty())
        .unwrap_or_else(|| String::from("unknown JavaScript error"))
}

fn load_files(files: Vec<FileData>, mut input: Signal<String>, mut file_status: Signal<String>) {
    if files.is_empty() {
        file_status.set(String::from("No files selected."));
        return;
    }

    spawn(async move {
        let file_count = files.len();
        let mut loaded = Vec::with_capacity(file_count);
        let mut errors = Vec::new();

        for file in files {
            let file_name = file.name();
            match file.read_string().await {
                Ok(text) if text.trim().is_empty() => {
                    errors.push(format!("{file_name} was empty"));
                }
                Ok(text) => loaded.push(text),
                Err(error) => errors.push(format!("{file_name}: {error}")),
            }
        }

        if loaded.is_empty() {
            file_status.set(format!(
                "No readable MGF text was loaded. {}",
                errors.first().map_or("", String::as_str)
            ));
            return;
        }

        input.set(loaded.join("\n\n"));
        let suffix = if file_count == 1 { "" } else { "s" };
        if errors.is_empty() {
            file_status.set(format!("Loaded {file_count} file{suffix}."));
        } else {
            let first_error = errors
                .first()
                .map_or_else(|| String::from("Unknown file read error."), Clone::clone);
            file_status.set(format!(
                "Loaded {} of {file_count} files. {first_error}",
                loaded.len(),
            ));
        }
    });
}

fn app_icon<T>(shape: T, _title: &'static str) -> Element
where
    T: IconShape + Clone + PartialEq + 'static,
{
    rsx! {
        span { aria_hidden: "true",
            Icon {
                class: "app-icon",
                height: 18_u32,
                width: 18_u32,
                icon: shape,
                title: None,
            }
        }
    }
}

fn format_optional_str(value: Option<&str>) -> String {
    value.map_or_else(|| String::from("-"), ToOwned::to_owned)
}

#[cfg(target_arch = "wasm32")]
fn download_tsv(text: &str) -> Result<(), String> {
    download_text_file(text, "mgf-splash.tsv", "TSV")
}

#[cfg(target_arch = "wasm32")]
fn download_mgf(text: &str) -> Result<(), String> {
    download_text_file(text, "mgf-splash-with-splash.mgf", "MGF")
}

#[cfg(target_arch = "wasm32")]
fn download_text_file(text: &str, filename: &str, label: &str) -> Result<(), String> {
    let window =
        web_sys::window().ok_or_else(|| String::from("Download is unavailable in this build."))?;
    let document = window
        .document()
        .ok_or_else(|| String::from("Download is unavailable in this build."))?;
    let body = document
        .body()
        .ok_or_else(|| String::from("Download is unavailable in this build."))?;

    let parts = Array::new();
    parts.push(&JsValue::from_str(text));
    let blob = Blob::new_with_str_sequence(&parts)
        .map_err(|error| format!("failed to prepare {label}: {}", js_error_text(&error)))?;
    let url = Url::create_object_url_with_blob(&blob).map_err(|error| {
        format!(
            "failed to prepare {label} download: {}",
            js_error_text(&error)
        )
    })?;

    let anchor = document
        .create_element("a")
        .map_err(|error| format!("failed to create download link: {}", js_error_text(&error)))?
        .dyn_into::<HtmlAnchorElement>()
        .map_err(|error| format!("failed to create download link: {}", js_error_text(&error)))?;
    anchor.set_href(&url);
    anchor.set_download(filename);

    body.append_child(anchor.as_ref()).map_err(|error| {
        format!(
            "failed to start {label} download: {}",
            js_error_text(&error)
        )
    })?;
    anchor.click();
    let _ = body.remove_child(anchor.as_ref());
    Url::revoke_object_url(&url).map_err(|error| {
        format!(
            "failed to clean up {label} download: {}",
            js_error_text(&error)
        )
    })?;

    Ok(())
}

#[cfg(not(target_arch = "wasm32"))]
fn download_tsv(_text: &str) -> Result<(), String> {
    Err(String::from("Download is unavailable in this build."))
}
