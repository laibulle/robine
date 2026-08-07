use leptos::prelude::*;
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_futures::{JsFuture, spawn_local};
use web_sys::{Request, RequestInit, Response, window};

fn main() {
    leptos::mount::mount_to_body(App);
}

#[component]
fn App() -> impl IntoView {
    let (health, set_health) = signal("Connexion à Robine…".to_string());
    let (token, set_token) = signal(String::new());
    let (output, set_output) = signal("Choisissez une action de secours.".to_string());
    let (password, set_password) = signal(String::new());
    let (authority, set_authority) = signal(String::new());
    let (certificate, set_certificate) = signal(String::new());
    let (fingerprint, set_fingerprint) = signal(String::new());
    let (entity_id, set_entity_id) = signal(String::new());
    let (turn_on, set_turn_on) = signal(true);

    spawn_local(async move {
        set_health.set(match get("/health", None).await {
            Ok(value) => format!("Serveur prêt — {value}"),
            Err(error) => format!("Robine ne répond pas : {error}"),
        });
    });

    let bootstrap = move |_| {
        let password = password.get();
        spawn_local(async move {
            match post(
                "/api/v1/setup/administrator",
                &serde_json::json!({ "password": password }).to_string(),
                None,
            )
            .await
            {
                Ok(value) => {
                    if let Ok(value) = serde_json::from_str::<serde_json::Value>(&value) {
                        if let Some(token) = value.get("token").and_then(serde_json::Value::as_str)
                        {
                            set_token.set(token.into());
                            set_output.set(
                                "Accès administrateur créé. Conservez ce jeton dans l’app native."
                                    .into(),
                            );
                            return;
                        }
                    }
                    set_output.set(value);
                }
                Err(error) => set_output.set(error),
            }
        });
    };
    let discover_hue = move |_| {
        let token = token.get();
        spawn_local(async move {
            set_output.set(
                get("/api/v1/adapters/hue/discover", Some(token))
                    .await
                    .unwrap_or_else(|error| error),
            );
        });
    };
    let pair_hue = move |_| {
        let token = token.get();
        let authority = authority.get();
        let certificate = certificate.get();
        let fingerprint = fingerprint.get();
        spawn_local(async move {
            set_output.set(post("/api/v1/adapters/hue/pair", &serde_json::json!({ "authority": authority, "certificate_pem": certificate, "certificate_sha256": fingerprint }).to_string(), Some(token)).await.unwrap_or_else(|error| error));
        });
    };
    let list_devices = move |_| {
        let token = token.get();
        spawn_local(async move {
            set_output.set(
                get("/api/v1/devices", Some(token))
                    .await
                    .unwrap_or_else(|error| error),
            );
        });
    };
    let command = move |_| {
        let token = token.get();
        let entity = entity_id.get();
        let on = turn_on.get();
        spawn_local(async move {
            set_output.set(
                post(
                    &format!("/api/v1/entities/{entity}/commands"),
                    &serde_json::json!({ "key": "switch", "value": on }).to_string(),
                    Some(token),
                )
                .await
                .unwrap_or_else(|error| error),
            );
        });
    };

    view! {
        <main class="shell">
            <header class="hero"><span class="paw" aria-hidden="true">"🐾"</span><div><p class="eyebrow">"CONSOLE DE SECOURS"</p><h1>"Robine"</h1><p>{move || health.get()}</p></div></header>
            <section class="card" aria-labelledby="access-title"><h2 id="access-title">"Accès local"</h2><p>"Le jeton reste seulement dans cette page ouverte ; l’app native le conserve dans le trousseau."</p><label>"Mot de passe administrateur"<input type="password" prop:value=move || password.get() on:input=move |ev| set_password.set(event_target_value(&ev)) /></label><button on:click=bootstrap>"Créer l’accès initial"</button><label>"Jeton local"<input autocomplete="off" prop:value=move || token.get() on:input=move |ev| set_token.set(event_target_value(&ev)) /></label></section>
            <section class="card" aria-labelledby="hue-title"><h2 id="hue-title">"Philips Hue"</h2><p>"Découvrez le bridge, confirmez son certificat, puis appuyez sur son bouton physique."</p><button on:click=discover_hue>"Chercher un bridge"</button><label>"Adresse du bridge"<input placeholder="192.168.1.20" prop:value=move || authority.get() on:input=move |ev| set_authority.set(event_target_value(&ev)) /></label><label>"Certificat PEM"<textarea prop:value=move || certificate.get() on:input=move |ev| set_certificate.set(event_target_value(&ev)) /></label><label>"Empreinte SHA-256"<input prop:value=move || fingerprint.get() on:input=move |ev| set_fingerprint.set(event_target_value(&ev)) /></label><button on:click=pair_hue>"Associer le bridge"</button></section>
            <section class="card" aria-labelledby="recovery-title"><h2 id="recovery-title">"Récupération"</h2><button on:click=list_devices>"Voir les appareils"</button><label>"Identifiant de l’élément"<input placeholder="UUID" prop:value=move || entity_id.get() on:input=move |ev| set_entity_id.set(event_target_value(&ev)) /></label><label class="toggle"><input type="checkbox" prop:checked=move || turn_on.get() on:change=move |ev| set_turn_on.set(event_target_checked(&ev)) />"Allumer"</label><button on:click=command>"Demander la commande"</button></section>
            <section class="result" aria-live="polite"><h2>"Réponse de Robine"</h2><pre>{move || output.get()}</pre></section>
        </main>
    }
}

async fn get(path: &str, token: Option<String>) -> Result<String, String> {
    request(path, "GET", None, token).await
}
async fn post(path: &str, body: &str, token: Option<String>) -> Result<String, String> {
    request(path, "POST", Some(body), token).await
}
async fn request(
    path: &str,
    method: &str,
    body: Option<&str>,
    token: Option<String>,
) -> Result<String, String> {
    let init = RequestInit::new();
    init.set_method(method);
    if let Some(body) = body {
        init.set_body(&JsValue::from_str(body));
    }
    let request = Request::new_with_str_and_init(path, &init).map_err(js_error)?;
    request
        .headers()
        .set("Accept", "application/json")
        .map_err(js_error)?;
    if body.is_some() {
        request
            .headers()
            .set("Content-Type", "application/json")
            .map_err(js_error)?;
    }
    if let Some(token) = token.filter(|token| !token.is_empty()) {
        request
            .headers()
            .set("Authorization", &format!("Bearer {token}"))
            .map_err(js_error)?;
    }
    if method == "POST" && path.contains("/commands") {
        request
            .headers()
            .set("Idempotency-Key", &format!("web-{}", js_sys::Date::now()))
            .map_err(js_error)?;
    }
    let response = JsFuture::from(
        window()
            .ok_or_else(|| "browser window unavailable".to_string())?
            .fetch_with_request(&request),
    )
    .await
    .map_err(js_error)?
    .dyn_into::<Response>()
    .map_err(js_error)?;
    let text = JsFuture::from(response.text().map_err(js_error)?)
        .await
        .map_err(js_error)?
        .as_string()
        .unwrap_or_default();
    if response.ok() {
        Ok(text)
    } else {
        Err(format!(
            "La demande a été refusée ({}) : {text}",
            response.status()
        ))
    }
}
fn js_error(value: JsValue) -> String {
    value
        .as_string()
        .unwrap_or_else(|| "erreur navigateur inconnue".into())
}
