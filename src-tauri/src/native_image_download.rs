//! Explicit engine downloads use the page's WebKit session without navigating it.
use tauri::Webview;

pub fn start(webview: &Webview, url: &str) -> Result<(), String> {
    if let Some(reason) = neurobrowser::netguard::blocked_reason(url) {
        return Err(reason.to_string());
    }
    start_native(webview, url)
}

#[cfg(target_os = "macos")]
fn start_native(webview: &Webview, url: &str) -> Result<(), String> {
    let owner = webview.clone();
    let url = url.to_string();
    webview
        .with_webview(move |platform| unsafe {
            use objc2::runtime::ProtocolObject;
            use objc2::{msg_send, runtime::AnyObject, sel};
            use objc2_foundation::{NSObjectProtocol, NSString, NSURLRequest, NSURL};
            use objc2_web_kit::{WKDownload, WKDownloadDelegate, WKWebView};
            let view = &*(platform.inner() as *const WKWebView);
            if !view.respondsToSelector(sel!(startDownloadUsingRequest:completionHandler:)) {
                tracing::warn!("native image download requires macOS 11.3 or later");
                return;
            }
            let Some(url) = NSURL::URLWithString(&NSString::from_str(&url)) else {
                return;
            };
            let request = NSURLRequest::requestWithURL(&url);
            let callback = block2::RcBlock::new(move |download: std::ptr::NonNull<WKDownload>| {
                let download = download.as_ref();
                let delegate = macos::ImageDownloadDelegate::new(owner.clone());
                // WKDownload's delegate is weak. Associate it with the download so
                // both live until WebKit releases the completed download.
                unsafe extern "C" {
                    fn objc_setAssociatedObject(
                        object: *const AnyObject,
                        key: *const std::ffi::c_void,
                        value: *const AnyObject,
                        policy: usize,
                    );
                }
                static KEY: u8 = 0;
                objc_setAssociatedObject(
                    download as *const _ as *const AnyObject,
                    &KEY as *const _ as *const _,
                    &*delegate as *const _ as *const AnyObject,
                    1,
                );
                download.setDelegate(Some(ProtocolObject::<dyn WKDownloadDelegate>::from_ref(
                    &*delegate,
                )));
                let _: () = msg_send![download, retain];
                // Balanced at terminal callbacks. WebKit otherwise does not promise
                // to retain a programmatically initiated download for its caller.
            });
            view.startDownloadUsingRequest_completionHandler(&request, &callback);
        })
        .map_err(|error| error.to_string())
}

#[cfg(any(
    target_os = "linux",
    target_os = "dragonfly",
    target_os = "freebsd",
    target_os = "netbsd",
    target_os = "openbsd"
))]
fn start_native(webview: &Webview, url: &str) -> Result<(), String> {
    use webkit2gtk::WebViewExt;
    let url = url.to_string();
    webview
        .with_webview(move |platform| {
            platform.inner().download_uri(&url);
        })
        .map_err(|error| error.to_string())
}

#[cfg(not(any(
    target_os = "macos",
    target_os = "linux",
    target_os = "dragonfly",
    target_os = "freebsd",
    target_os = "netbsd",
    target_os = "openbsd"
)))]
fn start_native(_webview: &Webview, _url: &str) -> Result<(), String> {
    Err("Native image downloads are unavailable on this platform".into())
}

#[cfg(target_os = "macos")]
mod macos {
    use objc2::{define_class, msg_send, rc::Retained, DeclaredClass, MainThreadOnly};
    use objc2_foundation::{
        NSData, NSError, NSHTTPURLResponse, NSObject, NSObjectProtocol, NSString, NSURLRequest,
        NSURLResponse, NSURL,
    };
    use objc2_web_kit::{WKDownload, WKDownloadDelegate, WKDownloadRedirectPolicy};
    use std::{cell::RefCell, path::PathBuf};
    use tauri::{webview::DownloadEvent, Webview};

    pub struct DownloadState {
        webview: Webview,
        destination: RefCell<Option<PathBuf>>,
    }
    define_class!(
        #[unsafe(super(NSObject))]
        #[name = "NeuroImageDownloadDelegate"]
        #[thread_kind = MainThreadOnly]
        #[ivars = DownloadState]
        pub struct ImageDownloadDelegate;
        unsafe impl NSObjectProtocol for ImageDownloadDelegate {}
        unsafe impl WKDownloadDelegate for ImageDownloadDelegate {
            #[unsafe(method(download:decideDestinationUsingResponse:suggestedFilename:completionHandler:))]
            fn destination(
                &self,
                download: &WKDownload,
                _response: &NSURLResponse,
                filename: &NSString,
                handler: &block2::Block<dyn Fn(*mut NSURL)>,
            ) {
                unsafe {
                    if response_is_blocked(_response) {
                        handler.call((std::ptr::null_mut(),));
                        return;
                    }
                    let Some(url) = original_url(download) else {
                        handler.call((std::ptr::null_mut(),));
                        return;
                    };
                    let mut destination = PathBuf::from(filename.to_string());
                    if !crate::downloads::handle_download_event(
                        self.ivars().webview.clone(),
                        DownloadEvent::Requested {
                            url: url.clone(),
                            destination: &mut destination,
                        },
                    ) {
                        handler.call((std::ptr::null_mut(),));
                        return;
                    }
                    let native =
                        NSURL::fileURLWithPath(&NSString::from_str(&destination.to_string_lossy()));
                    *self.ivars().destination.borrow_mut() = Some(destination);
                    handler.call((Retained::as_ptr(&native).cast_mut(),));
                }
            }
            #[unsafe(method(download:willPerformHTTPRedirection:newRequest:decisionHandler:))]
            fn redirect(
                &self,
                _download: &WKDownload,
                _response: &NSHTTPURLResponse,
                request: &NSURLRequest,
                handler: &block2::Block<dyn Fn(WKDownloadRedirectPolicy)>,
            ) {
                let allowed = request
                    .URL()
                    .and_then(|url| url.absoluteString())
                    .is_some_and(|url| {
                        neurobrowser::netguard::blocked_reason(&url.to_string()).is_none()
                    });
                handler.call((if allowed {
                    WKDownloadRedirectPolicy::Allow
                } else {
                    WKDownloadRedirectPolicy::Cancel
                },));
            }
            #[unsafe(method(downloadDidFinish:))]
            fn finished(&self, download: &WKDownload) {
                self.complete(download, true);
            }
            #[unsafe(method(download:didFailWithError:resumeData:))]
            fn failed(&self, download: &WKDownload, _error: &NSError, _data: Option<&NSData>) {
                self.complete(download, false);
            }
        }
    );
    impl ImageDownloadDelegate {
        pub fn new(webview: Webview) -> Retained<Self> {
            let marker = objc2::MainThreadMarker::new().expect("WebKit main thread");
            let delegate = marker.alloc::<Self>().set_ivars(DownloadState {
                webview,
                destination: RefCell::new(None),
            });
            unsafe { msg_send![super(delegate), init] }
        }
        fn complete(&self, download: &WKDownload, success: bool) {
            if let Some(url) = unsafe { original_url(download) } {
                if let Some(event) =
                    completion_event(url, self.ivars().destination.borrow().clone(), success)
                {
                    crate::downloads::handle_download_event(self.ivars().webview.clone(), event);
                }
            }
            unsafe {
                let _: () = msg_send![download, release];
            }
        }
    }
    // A request that failed before destination approval has no registered row.
    // Sending a pathless finish would consume another same-URL download's row.
    fn completion_event(
        url: tauri::Url,
        path: Option<PathBuf>,
        success: bool,
    ) -> Option<DownloadEvent<'static>> {
        let path = path?;
        Some(DownloadEvent::Finished {
            url,
            path: Some(path),
            success,
        })
    }
    #[cfg(test)]
    mod tests {
        use super::*;
        #[test]
        fn early_failure_cannot_finish_another_same_url_download() {
            let url: tauri::Url = "https://8.8.8.8/image.png".parse().unwrap();
            assert!(completion_event(url.clone(), None, false).is_none());
            let path = PathBuf::from("/tmp/neurobrowser-image-download-test.png");
            let Some(DownloadEvent::Finished {
                path: Some(actual),
                success: false,
                ..
            }) = completion_event(url, Some(path.clone()), false)
            else {
                panic!("approved download must finish by its own destination");
            };
            assert_eq!(actual, path);
        }
    }
    fn response_is_blocked(response: &NSURLResponse) -> bool {
        response
            .URL()
            .and_then(|url| url.absoluteString())
            .is_none_or(|url| neurobrowser::netguard::blocked_reason(&url.to_string()).is_some())
    }
    unsafe fn original_url(download: &WKDownload) -> Option<tauri::Url> {
        download
            .originalRequest()?
            .URL()?
            .absoluteString()?
            .to_string()
            .parse()
            .ok()
    }
}
