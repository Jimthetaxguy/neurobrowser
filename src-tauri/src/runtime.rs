use async_trait::async_trait;
use neurobrowser::{browser::enrich_snapshot, BrowserInterface, ElementInfo, PageSnapshot};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tauri::webview::{PageLoadEvent, WebviewBuilder};
use tauri::{AppHandle, LogicalPosition, LogicalSize, Manager, Webview, WebviewUrl, Window};
use tokio::sync::oneshot;
use tokio::time::sleep;

const ABOUT_BLANK_URL: &str = "about:blank";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);
const RUNTIME_INIT_SCRIPT: &str = r#"
(() => {
  if (window.__NEUROBROWSER_RUNTIME__) {
    return;
  }

  const invoke = window.__TAURI_INTERNALS__ && window.__TAURI_INTERNALS__.invoke;

  const limitText = (value, max = 1000) => {
    if (typeof value !== 'string') {
      return '';
    }
    return value.trim().slice(0, max);
  };

  const attrsToObject = (element) => {
    const attrs = {};
    if (!element || !element.attributes) {
      return attrs;
    }
    for (const attr of Array.from(element.attributes)) {
      attrs[attr.name] = attr.value;
    }
    return attrs;
  };

  const serializeElement = (element, selector) => ({
    tag: element.tagName ? element.tagName.toLowerCase() : '',
    id: element.id || null,
    classes: Array.from(element.classList || []),
    text: limitText(element.innerText || element.textContent || '', 220),
    attributes: attrsToObject(element),
    selector
  });

  const collectForms = () =>
    Array.from(document.querySelectorAll('form')).slice(0, 40).map((form) => ({
      action: form.getAttribute('action') || '',
      method: form.getAttribute('method') || 'get',
      selector: 'form',
      inputs: Array.from(form.querySelectorAll('input, textarea, select, button'))
        .slice(0, 80)
        .map((input) => ({
          name: input.getAttribute('name') || '',
          input_type: input.getAttribute('type') || input.tagName.toLowerCase(),
          selector: input.tagName.toLowerCase(),
          value: typeof input.value === 'string' ? limitText(input.value, 200) : null
        }))
    }));

  const collectTables = () =>
    Array.from(document.querySelectorAll('table')).slice(0, 20).map((table) => ({
      headers: Array.from(table.querySelectorAll('th')).slice(0, 32).map((cell) => limitText(cell.innerText || cell.textContent || '', 120)),
      rows: Array.from(table.querySelectorAll('tr')).slice(0, 64).map((row) =>
        Array.from(row.querySelectorAll('td')).slice(0, 24).map((cell) => limitText(cell.innerText || cell.textContent || '', 120))
      ).filter((row) => row.length > 0),
      selector: 'table'
    }));

  const runtime = {
    dispatch(pageId, requestId, producer) {
      if (!invoke) {
        throw new Error('Tauri invoke bridge is not available in this webview');
      }

      Promise.resolve()
        .then(producer)
        .then((payload) => invoke('browser_runtime_report', {
          payload: {
            page_id: pageId,
            request_id: requestId,
            payload,
            error: null
          }
        }))
        .catch((error) => {
          const message = error && error.message ? error.message : String(error);
          return invoke('browser_runtime_report', {
            payload: {
              page_id: pageId,
              request_id: requestId,
              payload: null,
              error: message
            }
          });
        });
    },

    snapshot() {
      const root = document.documentElement;
      return {
        url: window.location.href,
        title: document.title || '',
        html: root ? limitText(root.outerHTML || '', 250000) : null,
        text: document.body ? limitText(document.body.innerText || document.body.textContent || '', 50000) : null,
        viewport_width: window.innerWidth || 0,
        viewport_height: window.innerHeight || 0,
        scroll_x: window.scrollX || 0,
        scroll_y: window.scrollY || 0,
        interactive_ready: document.readyState === 'interactive' || document.readyState === 'complete',
        links: Array.from(document.querySelectorAll('a[href]')).slice(0, 200).map((link) => ({
          href: link.href || '',
          text: limitText(link.innerText || link.textContent || '', 160),
          selector: 'a[href]'
        })),
        images: Array.from(document.querySelectorAll('img')).slice(0, 200).map((image) => ({
          src: image.currentSrc || image.src || '',
          alt: image.alt || '',
          width: Number.isFinite(image.naturalWidth) && image.naturalWidth > 0 ? image.naturalWidth : null,
          height: Number.isFinite(image.naturalHeight) && image.naturalHeight > 0 ? image.naturalHeight : null
        })),
        forms: collectForms(),
        prices: [],
        tables: collectTables()
      };
    },

    querySelector(selector) {
      return Array.from(document.querySelectorAll(selector)).slice(0, 100).map((element) => serializeElement(element, selector));
    },

    getText(selector) {
      return Array.from(document.querySelectorAll(selector))
        .slice(0, 100)
        .map((element) => limitText(element.innerText || element.textContent || '', 1000))
        .join('\n');
    },

    getAttributes(selector) {
      const element = document.querySelector(selector);
      if (!element) {
        throw new Error(`No element matched selector: ${selector}`);
      }
      return attrsToObject(element);
    },

    click(selector) {
      const element = document.querySelector(selector);
      if (!element) {
        throw new Error(`No element matched selector: ${selector}`);
      }
      element.click();
      return { ok: true };
    },

    typeText(selector, text) {
      const element = document.querySelector(selector);
      if (!element) {
        throw new Error(`No element matched selector: ${selector}`);
      }
      if (!('value' in element)) {
        throw new Error(`Element does not support value assignment: ${selector}`);
      }
      element.focus();
      element.value = text;
      element.dispatchEvent(new Event('input', { bubbles: true }));
      element.dispatchEvent(new Event('change', { bubbles: true }));
      return { ok: true };
    },

    submitForm(selector) {
      const target = document.querySelector(selector);
      if (!target) {
        throw new Error(`No element matched selector: ${selector}`);
      }
      const form = target.tagName && target.tagName.toLowerCase() === 'form'
        ? target
        : target.closest('form');
      if (!form) {
        throw new Error(`No form found for selector: ${selector}`);
      }
      if (typeof form.requestSubmit === 'function') {
        form.requestSubmit();
      } else {
        form.submit();
      }
      return { ok: true };
    },

    scrollTo(selector) {
      const element = document.querySelector(selector);
      if (!element) {
        throw new Error(`No element matched selector: ${selector}`);
      }
      element.scrollIntoView({ behavior: 'auto', block: 'center', inline: 'center' });
      return { ok: true };
    },

    scrollBy(x, y) {
      window.scrollBy(x, y);
      return { ok: true };
    },

    keypress(key) {
      const target = document.activeElement || document.body || document.documentElement;
      if (!target) {
        throw new Error('No focused element available for keypress');
      }
      const init = { key, bubbles: true, cancelable: true };
      target.dispatchEvent(new KeyboardEvent('keydown', init));
      target.dispatchEvent(new KeyboardEvent('keyup', init));
      return { ok: true };
    }
  };

  window.__NEUROBROWSER_RUNTIME__ = runtime;
})();
"#;

#[derive(Clone, Copy, Debug, Serialize, Deserialize, Default)]
pub struct BrowserViewport {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

struct RuntimePage {
    runtime_id: String,
    viewport: BrowserViewport,
    loading: bool,
    visible: bool,
    last_snapshot: Option<PageSnapshot>,
}

#[derive(Default)]
pub struct BrowserRuntimeRegistry {
    pages: Mutex<HashMap<usize, RuntimePage>>,
    pending: Mutex<HashMap<String, oneshot::Sender<Result<Value, String>>>>,
    active_page: Mutex<Option<usize>>,
}

impl BrowserRuntimeRegistry {
    pub fn register_page(&self, page_id: usize, runtime_id: String) {
        self.pages.lock().unwrap().insert(
            page_id,
            RuntimePage {
                runtime_id,
                viewport: BrowserViewport::default(),
                loading: false,
                visible: false,
                last_snapshot: None,
            },
        );
    }

    pub fn unregister_page(&self, page_id: usize) -> Option<String> {
        let removed = self.pages.lock().unwrap().remove(&page_id);
        let mut active_page = self.active_page.lock().unwrap();
        if active_page.as_ref() == Some(&page_id) {
            *active_page = None;
        }
        removed.map(|page| page.runtime_id)
    }

    pub fn page_runtime_id(&self, page_id: usize) -> Result<String, String> {
        self.pages
            .lock()
            .unwrap()
            .get(&page_id)
            .map(|page| page.runtime_id.clone())
            .ok_or_else(|| "Runtime page not found".to_string())
    }

    pub fn begin_request(&self) -> (String, oneshot::Receiver<Result<Value, String>>) {
        let request_id = uuid::Uuid::new_v4().to_string();
        let (sender, receiver) = oneshot::channel();
        self.pending
            .lock()
            .unwrap()
            .insert(request_id.clone(), sender);
        (request_id, receiver)
    }

    pub fn cancel_request(&self, request_id: &str) {
        self.pending.lock().unwrap().remove(request_id);
    }

    pub fn resolve_request(
        &self,
        request_id: &str,
        payload: Option<Value>,
        error: Option<String>,
    ) -> Result<(), String> {
        if let Some(sender) = self.pending.lock().unwrap().remove(request_id) {
            let _ = sender.send(match error {
                Some(message) => Err(message),
                None => Ok(payload.unwrap_or(Value::Null)),
            });
        }
        Ok(())
    }

    pub fn set_loading(&self, page_id: usize, loading: bool) {
        if let Some(page) = self.pages.lock().unwrap().get_mut(&page_id) {
            page.loading = loading;
        }
    }

    pub fn is_loading(&self, page_id: usize) -> Result<bool, String> {
        self.pages
            .lock()
            .unwrap()
            .get(&page_id)
            .map(|page| page.loading)
            .ok_or_else(|| "Runtime page not found".to_string())
    }

    pub fn set_viewport(&self, page_id: usize, viewport: BrowserViewport) -> Result<(), String> {
        let mut pages = self.pages.lock().unwrap();
        let page = pages
            .get_mut(&page_id)
            .ok_or_else(|| "Runtime page not found".to_string())?;
        page.viewport = viewport;
        Ok(())
    }

    pub fn viewport(&self, page_id: usize) -> Result<BrowserViewport, String> {
        self.pages
            .lock()
            .unwrap()
            .get(&page_id)
            .map(|page| page.viewport)
            .ok_or_else(|| "Runtime page not found".to_string())
    }

    pub fn set_visible(&self, page_id: usize, visible: bool) {
        if let Some(page) = self.pages.lock().unwrap().get_mut(&page_id) {
            page.visible = visible;
        }
    }

    pub fn set_active_page(&self, page_id: Option<usize>) {
        *self.active_page.lock().unwrap() = page_id;
    }

    pub fn active_page(&self) -> Option<usize> {
        *self.active_page.lock().unwrap()
    }

    pub fn all_pages(&self) -> Vec<(usize, String)> {
        self.pages
            .lock()
            .unwrap()
            .iter()
            .map(|(page_id, page)| (*page_id, page.runtime_id.clone()))
            .collect()
    }

    pub fn update_snapshot(&self, page_id: usize, snapshot: &PageSnapshot) {
        if let Some(page) = self.pages.lock().unwrap().get_mut(&page_id) {
            page.last_snapshot = Some(snapshot.clone());
        }
    }

    pub fn snapshot(&self, page_id: usize) -> Option<PageSnapshot> {
        self.pages
            .lock()
            .unwrap()
            .get(&page_id)
            .and_then(|page| page.last_snapshot.clone())
    }
}

#[derive(Deserialize)]
pub struct RuntimeReportPayload {
    pub page_id: usize,
    pub request_id: String,
    pub payload: Option<Value>,
    pub error: Option<String>,
}

#[derive(Clone)]
pub struct TauriBrowserRuntime {
    app: AppHandle,
    page_id: usize,
    runtime_id: String,
    registry: Arc<BrowserRuntimeRegistry>,
}

impl TauriBrowserRuntime {
    pub fn new(
        app: AppHandle,
        page_id: usize,
        runtime_id: String,
        registry: Arc<BrowserRuntimeRegistry>,
    ) -> Self {
        Self {
            app,
            page_id,
            runtime_id,
            registry,
        }
    }

    fn webview(&self) -> Result<Webview, String> {
        self.app
            .get_webview(&self.runtime_id)
            .ok_or_else(|| format!("Webview '{}' not found", self.runtime_id))
    }

    async fn request_json<T>(&self, script_expression: &str) -> Result<T, String>
    where
        T: DeserializeOwned,
    {
        let (request_id, receiver) = self.registry.begin_request();
        let request_id_js = serde_json::to_string(&request_id).map_err(|e| e.to_string())?;
        let script = format!(
            "(() => {{ const runtime = window.__NEUROBROWSER_RUNTIME__; if (!runtime) {{ throw new Error('NeuroBrowser runtime bridge is not installed'); }} runtime.dispatch({}, {}, () => ({})); }})();",
            self.page_id, request_id_js, script_expression
        );

        if let Err(error) = self.webview()?.eval(script) {
            self.registry.cancel_request(&request_id);
            return Err(error.to_string());
        }

        let value = match tokio::time::timeout(REQUEST_TIMEOUT, receiver).await {
            Ok(result) => result.map_err(|_| "Browser runtime response channel closed".to_string())??,
            Err(_) => {
                self.registry.cancel_request(&request_id);
                return Err(format!(
                    "Timed out waiting for browser runtime response for page {}",
                    self.page_id
                ));
            }
        };

        serde_json::from_value(value).map_err(|e| e.to_string())
    }

    async fn execute_action(&self, script_expression: &str) -> Result<(), String> {
        let _: Value = self.request_json(script_expression).await?;
        sleep(Duration::from_millis(120)).await;
        Ok(())
    }

    pub async fn wait_for_ready(&self, timeout_ms: u64) -> Result<(), String> {
        let deadline = Instant::now() + Duration::from_millis(timeout_ms);
        while Instant::now() <= deadline {
            if !self.registry.is_loading(self.page_id)? {
                return Ok(());
            }
            sleep(Duration::from_millis(40)).await;
        }
        Err(format!(
            "Timed out waiting for page {} to finish loading",
            self.page_id
        ))
    }
}

#[async_trait]
impl BrowserInterface for TauriBrowserRuntime {
    async fn navigate(&self, url: &str) -> Result<(), String> {
        let parsed = url.parse::<tauri::Url>().map_err(|e| e.to_string())?;
        self.registry.set_loading(self.page_id, true);
        self.webview()?.navigate(parsed).map_err(|e| e.to_string())
    }

    async fn query_selector(&self, selector: &str) -> Result<Vec<ElementInfo>, String> {
        let selector_json = serde_json::to_string(selector).map_err(|e| e.to_string())?;
        self.request_json(&format!("runtime.querySelector({selector_json})"))
            .await
    }

    async fn get_text(&self, selector: &str) -> Result<String, String> {
        let selector_json = serde_json::to_string(selector).map_err(|e| e.to_string())?;
        self.request_json(&format!("runtime.getText({selector_json})"))
            .await
    }

    async fn get_attributes(&self, selector: &str) -> Result<HashMap<String, String>, String> {
        let selector_json = serde_json::to_string(selector).map_err(|e| e.to_string())?;
        self.request_json(&format!("runtime.getAttributes({selector_json})"))
            .await
    }

    async fn click(&self, selector: &str) -> Result<(), String> {
        let selector_json = serde_json::to_string(selector).map_err(|e| e.to_string())?;
        self.execute_action(&format!("runtime.click({selector_json})"))
            .await?;
        let _ = self.wait_for_navigation().await;
        Ok(())
    }

    async fn type_text(&self, selector: &str, text: &str) -> Result<(), String> {
        let selector_json = serde_json::to_string(selector).map_err(|e| e.to_string())?;
        let text_json = serde_json::to_string(text).map_err(|e| e.to_string())?;
        self.execute_action(&format!("runtime.typeText({selector_json}, {text_json})"))
            .await
    }

    async fn submit_form(&self, selector: &str) -> Result<(), String> {
        let selector_json = serde_json::to_string(selector).map_err(|e| e.to_string())?;
        self.execute_action(&format!("runtime.submitForm({selector_json})"))
            .await?;
        let _ = self.wait_for_navigation().await;
        Ok(())
    }

    async fn scroll_to(&self, selector: &str) -> Result<(), String> {
        let selector_json = serde_json::to_string(selector).map_err(|e| e.to_string())?;
        self.execute_action(&format!("runtime.scrollTo({selector_json})"))
            .await
    }

    async fn scroll_by(&self, x: f32, y: f32) -> Result<(), String> {
        self.execute_action(&format!("runtime.scrollBy({}, {})", x, y))
            .await
    }

    async fn keypress(&self, key: &str) -> Result<(), String> {
        let key_json = serde_json::to_string(key).map_err(|e| e.to_string())?;
        self.execute_action(&format!("runtime.keypress({key_json})"))
            .await?;
        let _ = self.wait_for_navigation().await;
        Ok(())
    }

    async fn browser_back(&self) -> Result<(), String> {
        self.registry.set_loading(self.page_id, true);
        self.webview()?
            .eval("history.back()")
            .map_err(|e| e.to_string())?;
        let _ = self.wait_for_navigation().await;
        Ok(())
    }

    async fn browser_forward(&self) -> Result<(), String> {
        self.registry.set_loading(self.page_id, true);
        self.webview()?
            .eval("history.forward()")
            .map_err(|e| e.to_string())?;
        let _ = self.wait_for_navigation().await;
        Ok(())
    }

    async fn browser_reload(&self) -> Result<(), String> {
        self.registry.set_loading(self.page_id, true);
        self.webview()?.reload().map_err(|e| e.to_string())?;
        let _ = self.wait_for_navigation().await;
        Ok(())
    }

    async fn snapshot(&self) -> Result<PageSnapshot, String> {
        let mut snapshot: PageSnapshot = self.request_json("runtime.snapshot()").await?;
        enrich_snapshot(&mut snapshot);
        self.registry.update_snapshot(self.page_id, &snapshot);
        Ok(snapshot)
    }

    async fn wait_for_navigation(&self) -> Result<(), String> {
        self.wait_for_ready(3_000).await.or_else(|_| Ok(()))
    }
}

pub fn create_runtime_page(
    window: &Window,
    registry: Arc<BrowserRuntimeRegistry>,
    page_id: usize,
    runtime_id: &str,
) -> Result<(), String> {
    let external_url = ABOUT_BLANK_URL
        .parse::<tauri::Url>()
        .map_err(|e| e.to_string())?;
    let registry_for_nav = registry.clone();
    let registry_for_load = registry.clone();
    let registry_for_title = registry.clone();

    let builder = WebviewBuilder::new(runtime_id, WebviewUrl::External(external_url))
        .initialization_script(RUNTIME_INIT_SCRIPT)
        .on_navigation(move |_url| {
            registry_for_nav.set_loading(page_id, true);
            true
        })
        .on_page_load(move |_webview, payload| match payload.event() {
            PageLoadEvent::Started => registry_for_load.set_loading(page_id, true),
            PageLoadEvent::Finished => registry_for_load.set_loading(page_id, false),
        })
        .on_document_title_changed(move |_webview, _title| {
            if let Some(snapshot) = registry_for_title.snapshot(page_id) {
                registry_for_title.update_snapshot(page_id, &snapshot);
            }
        });

    let webview = window
        .add_child(
            builder,
            LogicalPosition::new(0.0, 0.0),
            LogicalSize::new(1.0, 1.0),
        )
        .map_err(|e| e.to_string())?;

    webview.set_auto_resize(false).map_err(|e| e.to_string())?;
    webview.hide().map_err(|e| e.to_string())?;
    registry.register_page(page_id, runtime_id.to_string());
    Ok(())
}

pub fn close_runtime_page(
    app: &AppHandle,
    registry: &BrowserRuntimeRegistry,
    page_id: usize,
) -> Result<(), String> {
    if let Some(runtime_id) = registry.unregister_page(page_id) {
        if let Some(webview) = app.get_webview(&runtime_id) {
            webview.close().map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

pub fn set_active_runtime_page(
    app: &AppHandle,
    registry: &BrowserRuntimeRegistry,
    page_id: usize,
) -> Result<(), String> {
    registry.set_active_page(Some(page_id));
    for (known_page_id, runtime_id) in registry.all_pages() {
        let webview = app
            .get_webview(&runtime_id)
            .ok_or_else(|| format!("Webview '{}' not found", runtime_id))?;
        if known_page_id == page_id {
            let viewport = registry.viewport(known_page_id)?;
            webview
                .set_position(LogicalPosition::new(viewport.x, viewport.y))
                .map_err(|e| e.to_string())?;
            webview
                .set_size(LogicalSize::new(
                    viewport.width.max(1.0),
                    viewport.height.max(1.0),
                ))
                .map_err(|e| e.to_string())?;
            webview.show().map_err(|e| e.to_string())?;
            registry.set_visible(known_page_id, true);
        } else {
            webview.hide().map_err(|e| e.to_string())?;
            registry.set_visible(known_page_id, false);
        }
    }
    Ok(())
}

pub fn sync_runtime_viewport(
    app: &AppHandle,
    registry: &BrowserRuntimeRegistry,
    page_id: usize,
    viewport: BrowserViewport,
) -> Result<(), String> {
    registry.set_viewport(page_id, viewport)?;
    if registry.active_page() == Some(page_id) {
        let runtime_id = registry.page_runtime_id(page_id)?;
        let webview = app
            .get_webview(&runtime_id)
            .ok_or_else(|| format!("Webview '{}' not found", runtime_id))?;
        webview
            .set_position(LogicalPosition::new(viewport.x, viewport.y))
            .map_err(|e| e.to_string())?;
        webview
            .set_size(LogicalSize::new(
                viewport.width.max(1.0),
                viewport.height.max(1.0),
            ))
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

pub async fn wait_for_runtime_page(
    registry: &BrowserRuntimeRegistry,
    page_id: usize,
    timeout_ms: u64,
) -> Result<(), String> {
    let deadline = Instant::now() + Duration::from_millis(timeout_ms);
    while Instant::now() <= deadline {
        if !registry.is_loading(page_id)? {
            return Ok(());
        }
        sleep(Duration::from_millis(40)).await;
    }
    Err(format!(
        "Timed out waiting for page {} to finish loading",
        page_id
    ))
}
