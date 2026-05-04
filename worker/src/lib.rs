//! Web worker entry point for browser-side SPLASH computation.

#[cfg(target_arch = "wasm32")]
mod app {
    use std::cell::Cell;

    use gloo_timers::future::TimeoutFuture;
    use js_sys::global;
    use mgf_splash_app::{
        MgfWorkerRequest, MgfWorkerResponse, mgf_with_splash, splash_report_from_mgf,
    };
    use wasm_bindgen::{JsCast, JsValue, closure::Closure, prelude::wasm_bindgen};
    use wasm_bindgen_futures::spawn_local;
    use web_sys::{DedicatedWorkerGlobalScope, MessageEvent};

    thread_local! {
        static ACTIVE_TOKEN: Cell<u64> = const { Cell::new(0) };
    }

    /// Initializes the worker message handler.
    #[wasm_bindgen(start)]
    fn start() {
        let scope = worker_scope();
        let onmessage_callback: Box<dyn FnMut(MessageEvent)> = Box::new(move |event| {
            let request = match serde_wasm_bindgen::from_value::<MgfWorkerRequest>(event.data()) {
                Ok(request) => request,
                Err(error) => {
                    let _ = post_response(&MgfWorkerResponse::Fatal {
                        token: ACTIVE_TOKEN.with(Cell::get),
                        message: format!("invalid worker request: {error}"),
                    });
                    return;
                }
            };

            match request {
                MgfWorkerRequest::Cancel { token } => {
                    ACTIVE_TOKEN.with(|active| active.set(token));
                }
                MgfWorkerRequest::Process { token, input } => {
                    ACTIVE_TOKEN.with(|active| active.set(token));
                    spawn_local(async move {
                        if post_response(&MgfWorkerResponse::Progress {
                            token,
                            label: String::from("Processing MGF spectra"),
                        })
                        .is_err()
                        {
                            return;
                        }

                        TimeoutFuture::new(0).await;
                        if is_stale(token) {
                            return;
                        }

                        match splash_report_from_mgf(&input) {
                            Ok(report) if !is_stale(token) => {
                                let _ =
                                    post_response(&MgfWorkerResponse::Complete { token, report });
                            }
                            Err(error) if !is_stale(token) => {
                                let _ = post_response(&MgfWorkerResponse::Fatal {
                                    token,
                                    message: error.message().to_owned(),
                                });
                            }
                            _ => {}
                        }
                    });
                }
                MgfWorkerRequest::AnnotateMgf { token, input } => {
                    ACTIVE_TOKEN.with(|active| active.set(token));
                    spawn_local(async move {
                        TimeoutFuture::new(0).await;
                        if is_stale(token) {
                            return;
                        }

                        match mgf_with_splash(&input) {
                            Ok(document) if !is_stale(token) => {
                                let _ = post_response(&MgfWorkerResponse::AnnotatedMgf {
                                    token,
                                    document,
                                });
                            }
                            Err(error) if !is_stale(token) => {
                                let _ = post_response(&MgfWorkerResponse::AnnotationFatal {
                                    token,
                                    message: error.message().to_owned(),
                                });
                            }
                            _ => {}
                        }
                    });
                }
            }
        });
        let onmessage = Closure::wrap(onmessage_callback);

        scope.set_onmessage(Some(onmessage.as_ref().unchecked_ref()));
        let _ = post_response(&MgfWorkerResponse::Ready);
        onmessage.forget();
    }

    fn is_stale(token: u64) -> bool {
        ACTIVE_TOKEN.with(|active| active.get() != token)
    }

    fn post_response(response: &MgfWorkerResponse) -> Result<(), JsValue> {
        let payload = serde_wasm_bindgen::to_value(response)
            .map_err(|error| JsValue::from_str(&format!("invalid worker response: {error}")))?;
        worker_scope().post_message(&payload)
    }

    fn worker_scope() -> DedicatedWorkerGlobalScope {
        global().unchecked_into::<DedicatedWorkerGlobalScope>()
    }
}

#[cfg(not(target_arch = "wasm32"))]
mod app {}
