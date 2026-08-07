use leptos::prelude::*;
use serde::{Deserialize, Serialize};
use std::{
    cell::{Cell, RefCell},
    rc::{Rc, Weak},
};
use wasm_bindgen::{JsCast, JsValue, closure::Closure};
use wasm_bindgen_futures::{JsFuture, spawn_local};
use web_sys::{Event, MessageEvent, Request, RequestInit, Response, WebSocket, window};

#[derive(Clone, Debug, Deserialize)]
struct HueBridgeCandidate {
    name: String,
    host: String,
    addresses: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct HueRoomSuggestion {
    name: String,
    entity_ids: Vec<String>,
}

/// Un seul abonnement navigateur peut vivre à la fois. Le compteur de
/// génération rend inoffensifs les callbacks retardés d'un ancien socket.
#[derive(Clone, Default)]
struct StreamController {
    state: Rc<StreamState>,
}

#[derive(Default)]
struct StreamState {
    generation: Cell<u64>,
    connection: RefCell<Option<StreamConnection>>,
}

struct StreamConnection {
    socket: WebSocket,
    _onopen: Closure<dyn FnMut(Event)>,
    _onmessage: Closure<dyn FnMut(MessageEvent)>,
    _onclose: Closure<dyn FnMut(Event)>,
}

impl StreamController {
    fn begin(&self) -> u64 {
        let generation = self.state.generation.get().wrapping_add(1);
        self.state.generation.set(generation);
        if let Some(connection) = self.state.connection.borrow_mut().take() {
            let _ = connection.socket.close();
        }
        generation
    }

    fn is_current(&self, generation: u64) -> bool {
        self.state.generation.get() == generation
    }

    fn stop(&self) {
        self.begin();
    }
}

fn main() {
    leptos::mount::mount_to_body(App);
}

#[component]
fn App() -> impl IntoView {
    let (health, set_health) = signal("Connexion à Robine…".to_string());
    let (token, set_token) = signal(String::new());
    let (output, set_output) = signal("Choisissez une action de secours.".to_string());
    let (stream_status, set_stream_status) = signal("Flux temps réel inactif.".to_string());
    let (stream_active, set_stream_active) = signal(false);
    let stream_controller = StreamController::default();
    let (password, set_password) = signal(String::new());
    let (authority, set_authority) = signal(String::new());
    let (certificate, set_certificate) = signal(String::new());
    let (fingerprint, set_fingerprint) = signal(String::new());
    let (discovered_hue, set_discovered_hue) = signal(Vec::<HueBridgeCandidate>::new());
    let (hue_room_suggestions, set_hue_room_suggestions) = signal(Vec::<HueRoomSuggestion>::new());
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
            match get("/api/v1/adapters/hue/discover", Some(token)).await {
                Ok(value) => match serde_json::from_str::<Vec<HueBridgeCandidate>>(&value) {
                    Ok(bridges) => {
                        let count = bridges.len();
                        set_discovered_hue.set(bridges);
                        set_output.set(if count == 0 {
                            "Aucun bridge Hue découvert. Saisissez son adresse locale si vous la connaissez.".into()
                        } else {
                            format!("{count} bridge(s) trouvé(s). Choisissez-en un puis confirmez son certificat.")
                        });
                    }
                    Err(_) => set_output.set(value),
                },
                Err(error) => set_output.set(error),
            }
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
    let synchronize_hue = move |_| {
        let token = token.get();
        spawn_local(async move {
            set_output.set(
                post("/api/v1/adapters/hue/synchronize", "{}", Some(token))
                    .await
                    .unwrap_or_else(|error| error),
            );
        });
    };
    let start_realtime = {
        let controller = stream_controller.clone();
        move |_| {
            if stream_active.get() {
                return;
            }
            let generation = controller.begin();
            set_stream_active.set(true);
            connect_stream(
                controller.clone(),
                generation,
                token.get(),
                set_output,
                set_stream_status,
                set_stream_active,
                0,
            );
        }
    };
    let stop_realtime = {
        let controller = stream_controller.clone();
        move |_| {
            controller.stop();
            set_stream_active.set(false);
            set_stream_status.set("Flux temps réel arrêté.".into());
        }
    };
    let list_hue_room_suggestions = move |_| {
        let token = token.get();
        spawn_local(async move {
            match get("/api/v1/adapters/hue/rooms", Some(token)).await {
                Ok(value) => match serde_json::from_str::<Vec<HueRoomSuggestion>>(&value) {
                    Ok(suggestions) => {
                        let count = suggestions.len();
                        set_hue_room_suggestions.set(suggestions);
                        set_output.set(if count == 0 {
                            "Aucune pièce ou zone Hue avec lumière à importer.".into()
                        } else {
                            format!("{count} regroupement(s) Hue à importer explicitement.")
                        });
                    }
                    Err(_) => set_output.set(value),
                },
                Err(error) => set_output.set(error),
            }
        });
    };
    let import_hue_room = move |suggestion: HueRoomSuggestion| {
        let token = token.get();
        spawn_local(async move {
            match post(
                "/api/v1/adapters/hue/rooms/import",
                &serde_json::json!({ "suggestion": suggestion }).to_string(),
                Some(token),
            )
            .await
            {
                Ok(value) => {
                    set_hue_room_suggestions.update(|suggestions| {
                        suggestions.retain(|candidate| {
                            candidate.name != suggestion.name
                                || candidate.entity_ids != suggestion.entity_ids
                        });
                    });
                    set_output.set(format!("Pièce Robine créée : {value}"));
                }
                Err(error) => set_output.set(error),
            }
        });
    };
    let list_adapters = move |_| {
        let token = token.get();
        spawn_local(async move {
            set_output.set(
                get("/api/v1/adapters", Some(token))
                    .await
                    .unwrap_or_else(|error| error),
            );
        });
    };
    let create_backup = move |_| {
        let token = token.get();
        spawn_local(async move {
            set_output.set(
                post("/api/v1/backups", "{}", Some(token))
                    .await
                    .unwrap_or_else(|error| error),
            );
        });
    };
    let list_automations = move |_| {
        let token = token.get();
        spawn_local(async move {
            set_output.set(
                get("/api/v1/automations", Some(token))
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
            <section class="card" aria-labelledby="hue-title"><h2 id="hue-title">"Philips Hue"</h2><p>"Découvrez le bridge, confirmez son certificat, puis appuyez sur son bouton physique."</p><button on:click=discover_hue>"Chercher un bridge"</button><Show when=move || !discovered_hue.get().is_empty()><ul class="bridge-list" aria-label="Bridges Hue découverts"><For each=move || discovered_hue.get() key=|bridge| bridge.host.clone() children=move |bridge| { let host = bridge.host.clone(); let label = if bridge.addresses.is_empty() { bridge.name } else { format!("{} · {}", bridge.name, bridge.addresses.join(", ")) }; view! { <li><button on:click=move |_| set_authority.set(host.clone())>{label}</button></li> } } /></ul></Show><label>"Adresse du bridge"<input placeholder="192.168.1.20" prop:value=move || authority.get() on:input=move |ev| set_authority.set(event_target_value(&ev)) /></label><label>"Certificat PEM"<textarea aria-describedby="hue-certificate-help" prop:value=move || certificate.get() on:input=move |ev| set_certificate.set(event_target_value(&ev)) /></label><p id="hue-certificate-help" class="hint">"Le certificat est une identité publique du bridge ; sa clé d’application ne quitte jamais le serveur."</p><label>"Empreinte SHA-256"<input prop:value=move || fingerprint.get() on:input=move |ev| set_fingerprint.set(event_target_value(&ev)) /></label><button on:click=pair_hue>"Associer le bridge"</button><button class="secondary" on:click=synchronize_hue>"Resynchroniser Hue"</button><button class="secondary" on:click=list_hue_room_suggestions>"Proposer les pièces et zones Hue"</button><Show when=move || !hue_room_suggestions.get().is_empty()><p class="hint">"L’import reste explicite : chaque action crée une pièce Robine et y affecte seulement les lumières suggérées."</p><ul class="bridge-list" aria-label="Regroupements Hue suggérés"><For each=move || hue_room_suggestions.get() key=|suggestion| format!("{}:{}", suggestion.name, suggestion.entity_ids.join(",")) children=move |suggestion| { let label = format!("Importer {} ({} lumière(s))", suggestion.name, suggestion.entity_ids.len()); view! { <li><button on:click=move |_| import_hue_room(suggestion.clone())>{label}</button></li> } } /></ul></Show></section>
            <section class="card" aria-labelledby="system-title"><h2 id="system-title">"Système"</h2><p>"Consultez les connexions, exportez une sauvegarde vérifiée ou relisez les habitudes sans installer l’app native."</p><button on:click=list_adapters>"Voir les connexions"</button><button on:click=create_backup>"Créer une sauvegarde"</button><button on:click=list_automations>"Voir les habitudes"</button></section>
            <section class="card" aria-labelledby="stream-title"><h2 id="stream-title">"Flux temps réel"</h2><p>"La console ouvre une session navigateur HttpOnly de dix minutes : le jeton ne figure jamais dans l’URL."</p><button on:click=start_realtime disabled=move || stream_active.get()>"Suivre les événements"</button><button class="secondary" on:click=stop_realtime disabled=move || !stream_active.get()>"Arrêter le flux"</button><p class="hint" aria-live="polite">{move || stream_status.get()}</p></section>
            <section class="card" aria-labelledby="recovery-title"><h2 id="recovery-title">"Récupération"</h2><button on:click=list_devices>"Voir les appareils"</button><label>"Identifiant de l’élément"<input placeholder="UUID" prop:value=move || entity_id.get() on:input=move |ev| set_entity_id.set(event_target_value(&ev)) /></label><label class="toggle"><input type="checkbox" prop:checked=move || turn_on.get() on:change=move |ev| set_turn_on.set(event_target_checked(&ev)) />"Allumer"</label><button on:click=command>"Demander la commande"</button></section>
            <section class="result" aria-live="polite"><h2>"Réponse de Robine"</h2><pre>{move || output.get()}</pre></section>
        </main>
    }
}

fn connect_stream(
    controller: StreamController,
    generation: u64,
    token: String,
    output: WriteSignal<String>,
    status: WriteSignal<String>,
    active: WriteSignal<bool>,
    attempt: u8,
) {
    spawn_local(async move {
        match post("/api/v1/auth/stream-session", "{}", Some(token.clone())).await {
            Ok(_) if controller.is_current(generation) => {
                match open_stream(
                    controller.clone(),
                    generation,
                    token,
                    output,
                    status,
                    active,
                    attempt,
                ) {
                    Ok(()) => status.set("Connexion au flux temps réel…".into()),
                    Err(error) => {
                        active.set(false);
                        status.set(error);
                    }
                }
            }
            Ok(_) => {}
            Err(error) if controller.is_current(generation) => {
                active.set(false);
                status.set(error);
            }
            Err(_) => {}
        }
    });
}

fn open_stream(
    controller: StreamController,
    generation: u64,
    token: String,
    output: WriteSignal<String>,
    status: WriteSignal<String>,
    active: WriteSignal<bool>,
    attempt: u8,
) -> Result<(), String> {
    let window = window().ok_or_else(|| "browser window unavailable".to_string())?;
    let location = window.location();
    let scheme = if location.protocol().map_err(js_error)? == "https:" {
        "wss"
    } else {
        "ws"
    };
    let host = location.host().map_err(js_error)?;
    let socket = WebSocket::new(&format!("{scheme}://{host}/api/v1/stream")).map_err(js_error)?;

    let state = Rc::downgrade(&controller.state);
    let subscription_socket = socket.clone();
    let onopen = Closure::<dyn FnMut(Event)>::new(move |_: Event| {
        if !is_current_generation(&state, generation) {
            return;
        }
        let subscription = serde_json::json!({
            "type": "subscribe",
            "topics": ["state", "device", "area", "automation", "adapter", "command"]
        })
        .to_string();
        if subscription_socket.send_with_str(&subscription).is_ok() {
            status.set("Flux temps réel connecté.".into());
        }
    });
    socket.set_onopen(Some(onopen.as_ref().unchecked_ref()));

    let state = Rc::downgrade(&controller.state);
    let onmessage = Closure::<dyn FnMut(MessageEvent)>::new(move |event: MessageEvent| {
        if is_current_generation(&state, generation)
            && let Some(text) = event.data().as_string()
        {
            output.set(format!("Événement temps réel : {text}"));
        }
    });
    socket.set_onmessage(Some(onmessage.as_ref().unchecked_ref()));

    let reconnect_window = window.clone();
    let state = Rc::downgrade(&controller.state);
    let onclose = Closure::<dyn FnMut(Event)>::new(move |_: Event| {
        let Some(state) = state.upgrade() else {
            return;
        };
        if state.generation.get() != generation {
            return;
        }
        let next_attempt = attempt.saturating_add(1);
        let delay = (1_000i32).saturating_mul(1_i32 << next_attempt.min(5));
        status.set(format!(
            "Flux interrompu — nouvelle tentative dans {} s.",
            delay / 1_000
        ));
        let reconnect_token = token.clone();
        let reconnect_state: Weak<StreamState> = Rc::downgrade(&state);
        let reconnect = Closure::<dyn FnMut()>::once(move || {
            let Some(state) = reconnect_state.upgrade() else {
                return;
            };
            let controller = StreamController { state };
            if controller.is_current(generation) {
                connect_stream(
                    controller,
                    generation,
                    reconnect_token,
                    output,
                    status,
                    active,
                    next_attempt,
                );
            }
        });
        let _ = reconnect_window.set_timeout_with_callback_and_timeout_and_arguments_0(
            reconnect.as_ref().unchecked_ref(),
            delay,
        );
        reconnect.forget();
    });
    socket.set_onclose(Some(onclose.as_ref().unchecked_ref()));

    // Les callbacks vivent exactement aussi longtemps que le socket. `stop`
    // les libère et invalide leurs éventuels callbacks de fermeture.
    *controller.state.connection.borrow_mut() = Some(StreamConnection {
        socket,
        _onopen: onopen,
        _onmessage: onmessage,
        _onclose: onclose,
    });
    Ok(())
}

fn is_current_generation(state: &Weak<StreamState>, generation: u64) -> bool {
    state
        .upgrade()
        .is_some_and(|state| state.generation.get() == generation)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_stream_generation_invalidates_the_previous_reconnect_chain() {
        let controller = StreamController::default();
        let first = controller.begin();
        let second = controller.begin();

        assert_ne!(first, second);
        assert!(!controller.is_current(first));
        assert!(controller.is_current(second));

        controller.stop();
        assert!(!controller.is_current(second));
    }
}
