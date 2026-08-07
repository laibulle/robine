//! Adaptateur Philips Hue. Les types de l'API Hue restent à cette frontière.

use chrono::{DateTime, Utc};
use mdns_sd::{ServiceDaemon, ServiceEvent};
use robine_application::{ApplicationError, CommandDispatcher, HomeService};
use robine_domain::*;
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    io::{BufRead, BufReader},
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};
use thiserror::Error;

#[derive(Clone, Debug, PartialEq)]
pub struct HueLight {
    pub resource_id: String,
    pub name: String,
    pub on: bool,
    /// Valeur Hue 0..100, normalisée en pourcentage avant le domaine.
    pub brightness: Option<f64>,
    /// Température de couleur Hue, en mirek. Absente si la lampe ne l'expose pas.
    pub color_temperature_mirek: Option<u16>,
    /// Coordonnées CIE 1931 exposées par Hue. Elles restent confinées à cet
    /// adaptateur ; le domaine reçoit une couleur sRGB `#RRGGBB`.
    pub color_xy: Option<HueColor>,
}

/// Capteur Hue lu depuis une ressource v2. Son type exact et son identifiant
/// restent ici ; le coeur ne reçoit qu'une capacité et une valeur canonique.
#[derive(Clone, Debug, PartialEq)]
pub struct HueSensor {
    pub resource_id: String,
    pub name: String,
    pub key: String,
    pub value: StateValue,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HueColor {
    pub x: f64,
    pub y: f64,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct HueInventory {
    pub bridge_id: String,
    pub lights: Vec<HueLight>,
    pub sensors: Vec<HueSensor>,
    pub rooms: Vec<HueRoom>,
}

/// Une pièce ou zone Hue est une suggestion d'organisation uniquement. Son
/// identifiant de protocole ne quitte jamais cet adaptateur.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct HueRoom {
    pub name: String,
    pub light_resource_ids: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HueRoomSuggestion {
    pub name: String,
    pub entity_ids: Vec<EntityId>,
}

/// Candidat d'appairage publié par Bonjour/mDNS. La découverte ne contacte
/// aucun service Internet et ne contient aucun secret ni certificat.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HueBridgeCandidate {
    pub name: String,
    pub host: String,
    pub addresses: Vec<String>,
}

/// Cherche les bridges qui annoncent le service local Hue pendant une fenêtre
/// bornée. L'adresse est laissée sans port : l'association Hue V1 utilise
/// HTTPS (443), indépendamment du port éventuellement annoncé par mDNS.
pub fn discover_bridges(timeout: std::time::Duration) -> Result<Vec<HueBridgeCandidate>, HueError> {
    let daemon = ServiceDaemon::new().map_err(|error| HueError::Transport(error.to_string()))?;
    let receiver = daemon
        .browse("_hue._tcp.local.")
        .map_err(|error| HueError::Transport(error.to_string()))?;
    let deadline = std::time::Instant::now() + timeout;
    let mut candidates = HashMap::new();
    while let Some(remaining) = deadline.checked_duration_since(std::time::Instant::now()) {
        let Ok(event) = receiver.recv_timeout(remaining) else {
            break;
        };
        if let ServiceEvent::ServiceResolved(info) = event {
            let mut addresses: Vec<String> =
                info.addresses.iter().map(ToString::to_string).collect();
            addresses.sort();
            addresses.dedup();
            if !addresses.is_empty() {
                candidates.insert(
                    info.fullname.clone(),
                    HueBridgeCandidate {
                        name: info.fullname,
                        host: info.host,
                        addresses,
                    },
                );
            }
        }
    }
    daemon
        .shutdown()
        .map_err(|error| HueError::Transport(error.to_string()))?;
    let mut candidates: Vec<_> = candidates.into_values().collect();
    candidates.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(candidates)
}

#[derive(Clone, Debug, PartialEq)]
pub enum HueValue {
    On(bool),
    Brightness(f64),
    ColorTemperature(u16),
    Color(HueColor),
}

/// Mise à jour partielle normalisée depuis l'EventStream Hue v2.
#[derive(Clone, Debug, PartialEq)]
pub struct HueLightStateUpdate {
    pub resource_id: String,
    pub on: Option<bool>,
    pub brightness: Option<f64>,
    pub color_temperature_mirek: Option<u16>,
    pub color_xy: Option<HueColor>,
    pub source_at: Option<DateTime<Utc>>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct HueSensorStateUpdate {
    pub resource_id: String,
    pub key: String,
    pub value: StateValue,
    pub source_at: Option<DateTime<Utc>>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum HueStateUpdate {
    Light(HueLightStateUpdate),
    Sensor(HueSensorStateUpdate),
}

pub trait HueBridgeClient: Send + Sync {
    fn create_application_key(&self) -> Result<String, HueError>;
    fn inventory(&self) -> Result<HueInventory, HueError>;
    fn set_light_state(&self, resource_id: &str, value: HueValue) -> Result<(), HueError>;
}

#[derive(Debug, Error)]
pub enum HueError {
    #[error("the bridge button must be pressed before pairing")]
    LinkButtonNotPressed,
    #[error("bridge request failed: {0}")]
    Transport(String),
    #[error("unsupported Hue command")]
    UnsupportedCommand,
    #[error("the bridge response is malformed: {0}")]
    InvalidResponse(String),
    #[error("an application key is required for this Hue request")]
    MissingApplicationKey,
    #[error("Hue rejected the request: {0}")]
    Rejected(String),
}

/// Politique de reprise bornée du flux Hue. L'entropie est injectée par le
/// superviseur : cette petite valeur pure reste donc déterministe en test et
/// ne décide ni d'une horloge ni d'un thread à la frontière protocolaire.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HueReconnectBackoff {
    initial: Duration,
    maximum: Duration,
}

impl Default for HueReconnectBackoff {
    fn default() -> Self {
        Self {
            initial: Duration::from_secs(1),
            maximum: Duration::from_secs(60),
        }
    }
}

impl HueReconnectBackoff {
    /// Retourne un délai exponentiel plafonné, augmenté d'une gigue uniforme
    /// de 0 à 20 %. `consecutive_failures` commence à 1.
    pub fn delay(&self, consecutive_failures: u8, entropy: u16) -> Duration {
        let exponent = consecutive_failures.saturating_sub(1).min(63);
        let base_millis = self
            .initial
            .as_millis()
            .saturating_mul(1u128 << exponent)
            .min(self.maximum.as_millis());
        let jitter_limit =
            (base_millis / 5).min(self.maximum.as_millis().saturating_sub(base_millis));
        let jitter = jitter_limit.saturating_mul(u128::from(entropy)) / u128::from(u16::MAX);
        Duration::from_millis(u64::try_from(base_millis.saturating_add(jitter)).unwrap_or(u64::MAX))
    }
}

/// Client de bridge local Hue. Il ne désactive jamais la validation TLS : le
/// certificat du bridge doit être fourni par le parcours d'association, après
/// vérification de son empreinte par l'administrateur.
pub struct HueHttpBridgeClient {
    client: reqwest::blocking::Client,
    base_url: String,
    application_key: Option<String>,
}

impl HueHttpBridgeClient {
    pub fn with_pinned_certificate(
        authority: &str,
        certificate_pem: &[u8],
        application_key: Option<String>,
    ) -> Result<Self, HueError> {
        let authority = authority.trim().trim_matches('/');
        if authority.is_empty()
            || authority.chars().any(|character| {
                character.is_whitespace() || matches!(character, '/' | '@' | '?' | '#')
            })
        {
            return Err(HueError::Transport("invalid bridge authority".into()));
        }
        let certificate = reqwest::Certificate::from_pem(certificate_pem)
            .map_err(|error| HueError::Transport(error.to_string()))?;
        let client = reqwest::blocking::Client::builder()
            .add_root_certificate(certificate)
            .build()
            .map_err(|error| HueError::Transport(error.to_string()))?;
        Ok(Self {
            client,
            base_url: format!("https://{authority}"),
            application_key,
        })
    }

    fn v2_get(&self, path: &str) -> Result<Value, HueError> {
        let application_key = self
            .application_key
            .as_deref()
            .ok_or(HueError::MissingApplicationKey)?;
        let response = self
            .client
            .get(format!("{}{path}", self.base_url))
            .header("hue-application-key", application_key)
            .send()
            .map_err(|error| HueError::Transport(error.to_string()))?
            .error_for_status()
            .map_err(|error| HueError::Transport(error.to_string()))?;
        response
            .json()
            .map_err(|error| HueError::InvalidResponse(error.to_string()))
    }

    fn v2_put(&self, path: &str, payload: Value) -> Result<(), HueError> {
        let application_key = self
            .application_key
            .as_deref()
            .ok_or(HueError::MissingApplicationKey)?;
        let response = self
            .client
            .put(format!("{}{path}", self.base_url))
            .header("hue-application-key", application_key)
            .json(&payload)
            .send()
            .map_err(|error| HueError::Transport(error.to_string()))?
            .error_for_status()
            .map_err(|error| HueError::Transport(error.to_string()))?;
        let response: Value = response
            .json()
            .map_err(|error| HueError::InvalidResponse(error.to_string()))?;
        if let Some(errors) = response.get("errors").and_then(Value::as_array) {
            if !errors.is_empty() {
                return Err(HueError::Rejected(
                    serde_json::to_string(errors).unwrap_or_else(|_| "unknown Hue error".into()),
                ));
            }
        }
        Ok(())
    }

    fn bridge_id(&self) -> Result<String, HueError> {
        let response = self.v2_get("/clip/v2/resource/bridge")?;
        response
            .get("data")
            .and_then(Value::as_array)
            .and_then(|items| items.first())
            .and_then(|item| item.get("id"))
            .and_then(Value::as_str)
            .map(str::to_owned)
            .ok_or_else(|| HueError::InvalidResponse("bridge identifier is missing".into()))
    }

    /// Lit le flux SSE Hue dans un thread dédié. Le callback ne reçoit que des
    /// mises à jour normalisées ; ni le payload brut ni la clé d'application ne
    /// quittent l'infrastructure.
    pub fn listen_events(
        &self,
        mut on_updates: impl FnMut(Vec<HueStateUpdate>) -> Result<(), HueError>,
    ) -> Result<(), HueError> {
        let application_key = self
            .application_key
            .as_deref()
            .ok_or(HueError::MissingApplicationKey)?;
        let response = self
            .client
            .get(format!("{}/eventstream/clip/v2", self.base_url))
            .header("hue-application-key", application_key)
            .header("accept", "text/event-stream")
            .send()
            .map_err(|error| HueError::Transport(error.to_string()))?
            .error_for_status()
            .map_err(|error| HueError::Transport(error.to_string()))?;
        let mut data = String::new();
        for line in BufReader::new(response).lines() {
            let line = line.map_err(|error| HueError::Transport(error.to_string()))?;
            if let Some(fragment) = line.strip_prefix("data:") {
                data.push_str(fragment.trim_start());
                continue;
            }
            if line.is_empty() && !data.is_empty() {
                let updates = parse_v2_event(&data)?;
                data.clear();
                if !updates.is_empty() {
                    on_updates(updates)?;
                }
            }
        }
        Err(HueError::Transport("Hue event stream closed".into()))
    }
}

impl HueBridgeClient for HueHttpBridgeClient {
    fn create_application_key(&self) -> Result<String, HueError> {
        let response = self
            .client
            .post(format!("{}/api", self.base_url))
            .json(&json!({ "devicetype": "robine#server" }))
            .send()
            .map_err(|error| HueError::Transport(error.to_string()))?
            .error_for_status()
            .map_err(|error| HueError::Transport(error.to_string()))?;
        let response: Value = response
            .json()
            .map_err(|error| HueError::InvalidResponse(error.to_string()))?;
        let Some(entry) = response.as_array().and_then(|items| items.first()) else {
            return Err(HueError::InvalidResponse(
                "pairing response is not an array".into(),
            ));
        };
        if let Some(error) = entry.get("error") {
            if error.get("type").and_then(Value::as_i64) == Some(101) {
                return Err(HueError::LinkButtonNotPressed);
            }
            return Err(HueError::Rejected(error.to_string()));
        }
        entry
            .get("success")
            .and_then(|success| success.get("username"))
            .and_then(Value::as_str)
            .map(str::to_owned)
            .ok_or_else(|| {
                HueError::InvalidResponse("pairing response has no application key".into())
            })
    }

    fn inventory(&self) -> Result<HueInventory, HueError> {
        let bridge_id = self.bridge_id()?;
        let response = self.v2_get("/clip/v2/resource/light")?;
        let lights = response
            .get("data")
            .and_then(Value::as_array)
            .ok_or_else(|| HueError::InvalidResponse("light inventory has no data array".into()))?
            .iter()
            .map(|light| {
                let resource_id = light
                    .get("id")
                    .and_then(Value::as_str)
                    .ok_or_else(|| HueError::InvalidResponse("light has no id".into()))?
                    .to_owned();
                let name = light
                    .pointer("/metadata/name")
                    .and_then(Value::as_str)
                    .unwrap_or("Philips Hue light")
                    .to_owned();
                let on = light
                    .pointer("/on/on")
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                let brightness = light
                    .pointer("/dimming/brightness")
                    .and_then(Value::as_f64)
                    .map(|value| value.clamp(0.0, 100.0));
                let color_temperature_mirek = light
                    .pointer("/color_temperature/mirek")
                    .and_then(Value::as_u64)
                    .and_then(|value| u16::try_from(value).ok());
                let color_xy = hue_color_from_json(light.pointer("/color/xy"));
                Ok(HueLight {
                    resource_id,
                    name,
                    on,
                    brightness,
                    color_temperature_mirek,
                    color_xy,
                })
            })
            .collect::<Result<Vec<_>, HueError>>()?;
        // Ces capteurs V2 sont tous lecture seule. Un endpoint absent sur un
        // bridge plus ancien ne compromet donc jamais les lumières.
        let sensors = [
            "/clip/v2/resource/temperature",
            "/clip/v2/resource/motion",
            "/clip/v2/resource/device_power",
            "/clip/v2/resource/contact",
        ]
        .into_iter()
        .filter_map(|path| self.v2_get(path).ok())
        .flat_map(|response| {
            response
                .get("data")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default()
        })
        .filter_map(|resource| hue_sensor_from_resource(&resource))
        .collect();
        // Les anciens bridges peuvent ne pas exposer `room` ou `zone`. Cette
        // absence ne doit pas empêcher l'inventaire des lumières ni le
        // contrôle local ; les deux collections sont seulement des suggestions.
        let rooms = ["/clip/v2/resource/room", "/clip/v2/resource/zone"]
            .into_iter()
            .filter_map(|path| self.v2_get(path).ok())
            .flat_map(|response| {
                response
                    .get("data")
                    .and_then(Value::as_array)
                    .cloned()
                    .unwrap_or_default()
            })
            .filter_map(|grouping| hue_room_from_resource(&grouping))
            .collect();
        Ok(HueInventory {
            bridge_id,
            lights,
            sensors,
            rooms,
        })
    }

    fn set_light_state(&self, resource_id: &str, value: HueValue) -> Result<(), HueError> {
        let payload = match value {
            HueValue::On(on) => json!({ "on": { "on": on } }),
            HueValue::Brightness(brightness) => {
                json!({ "dimming": { "brightness": brightness.clamp(0.0, 100.0) } })
            }
            HueValue::ColorTemperature(mirek) => {
                json!({ "color_temperature": { "mirek": mirek } })
            }
            HueValue::Color(color) => {
                json!({ "color": { "xy": { "x": color.x, "y": color.y } } })
            }
        };
        self.v2_put(&format!("/clip/v2/resource/light/{resource_id}"), payload)
    }
}

/// Décode un événement `data:` complet provenant du flux SSE Hue v2. Le
/// lecteur réseau garde les frontières de trame SSE ; ce parseur reste pur et
/// testable sans bridge.
pub fn parse_v2_event(payload: &str) -> Result<Vec<HueStateUpdate>, HueError> {
    let events: Value = serde_json::from_str(payload)
        .map_err(|error| HueError::InvalidResponse(error.to_string()))?;
    let events = events
        .as_array()
        .ok_or_else(|| HueError::InvalidResponse("event payload is not an array".into()))?;
    let mut updates = Vec::new();
    for event in events {
        for resource in event
            .get("data")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let Some(resource_id) = resource.get("id").and_then(Value::as_str) else {
                continue;
            };
            let source_at = event
                .get("creationtime")
                .and_then(Value::as_str)
                .and_then(|time| DateTime::parse_from_rfc3339(time).ok())
                .map(|time| time.with_timezone(&Utc));
            if let Some(sensor) = hue_sensor_update_from_resource(resource, source_at) {
                updates.push(HueStateUpdate::Sensor(sensor));
                continue;
            }
            if resource.get("type").and_then(Value::as_str) != Some("light") {
                continue;
            }
            let on = resource.pointer("/on/on").and_then(Value::as_bool);
            let brightness = resource
                .pointer("/dimming/brightness")
                .and_then(Value::as_f64)
                .map(|value| value.clamp(0.0, 100.0));
            let color_temperature_mirek = resource
                .pointer("/color_temperature/mirek")
                .and_then(Value::as_u64)
                .and_then(|value| u16::try_from(value).ok());
            let color_xy = hue_color_from_json(resource.pointer("/color/xy"));
            if on.is_some()
                || brightness.is_some()
                || color_temperature_mirek.is_some()
                || color_xy.is_some()
            {
                updates.push(HueStateUpdate::Light(HueLightStateUpdate {
                    resource_id: resource_id.into(),
                    on,
                    brightness,
                    color_temperature_mirek,
                    color_xy,
                    source_at,
                }));
            }
        }
    }
    Ok(updates)
}

fn hue_sensor_from_resource(resource: &Value) -> Option<HueSensor> {
    let resource_id = resource.get("id")?.as_str()?.to_owned();
    let name = resource
        .pointer("/metadata/name")
        .and_then(Value::as_str)
        .filter(|name| !name.trim().is_empty())
        .unwrap_or("Philips Hue sensor")
        .to_owned();
    let update = hue_sensor_update_from_resource(resource, None)?;
    Some(HueSensor {
        resource_id,
        name,
        key: update.key,
        value: update.value,
    })
}

fn hue_sensor_update_from_resource(
    resource: &Value,
    source_at: Option<DateTime<Utc>>,
) -> Option<HueSensorStateUpdate> {
    let resource_id = resource.get("id")?.as_str()?.to_owned();
    let (key, value) = match resource.get("type")?.as_str()? {
        "temperature" => (
            "sensor.temperature",
            StateValue::Text(
                resource
                    .pointer("/temperature/temperature")?
                    .as_f64()?
                    .to_string(),
            ),
        ),
        "motion" => (
            "sensor.occupancy",
            StateValue::Bool(resource.pointer("/motion/motion")?.as_bool()?),
        ),
        "device_power" => (
            "sensor.battery",
            StateValue::Percentage(
                resource
                    .pointer("/power_state/battery_level")?
                    .as_f64()?
                    .clamp(0.0, 100.0),
            ),
        ),
        "contact" => (
            "sensor.binary",
            StateValue::Bool(
                resource
                    .pointer("/contact_report/state")?
                    .as_str()
                    .map(|state| state == "contact")?,
            ),
        ),
        _ => return None,
    };
    Some(HueSensorStateUpdate {
        resource_id,
        key: key.into(),
        value,
        source_at,
    })
}

fn hue_color_from_json(value: Option<&Value>) -> Option<HueColor> {
    let value = value?;
    let x = value.get("x")?.as_f64()?;
    let y = value.get("y")?.as_f64()?;
    (x.is_finite() && y.is_finite() && (0.0..=1.0).contains(&x) && (0.0..=1.0).contains(&y))
        .then_some(HueColor { x, y })
}

fn hue_room_from_resource(resource: &Value) -> Option<HueRoom> {
    let name = resource.pointer("/metadata/name")?.as_str()?.trim();
    if name.is_empty() {
        return None;
    }
    let mut light_resource_ids = resource
        .get("children")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|child| child.get("rtype").and_then(Value::as_str) == Some("light"))
        .filter_map(|child| child.get("rid").and_then(Value::as_str).map(str::to_owned))
        .collect::<Vec<_>>();
    light_resource_ids.sort();
    light_resource_ids.dedup();
    (!light_resource_ids.is_empty()).then_some(HueRoom {
        name: name.into(),
        light_resource_ids,
    })
}

/// Conversion entre le CIE xy Hue et le sRGB affiché par Robine. Cette
/// normalisation stable rend l'état portable entre adaptateurs ; l'adaptateur
/// Hue se charge ensuite du retour vers xy lors d'une commande.
fn srgb_hex_from_hue_color(color: HueColor) -> String {
    if color.y <= f64::EPSILON {
        return "#000000".into();
    }
    let x = color.x;
    let y = color.y;
    let z = (1.0 - x - y).max(0.0);
    let x_xyz = x / y;
    let z_xyz = z / y;
    let red = 3.2406 * x_xyz - 1.5372 - 0.4986 * z_xyz;
    let green = -0.9689 * x_xyz + 1.8758 + 0.0415 * z_xyz;
    let blue = 0.0557 * x_xyz - 0.2040 + 1.0570 * z_xyz;
    let encode = |value: f64| {
        let value = value.max(0.0);
        let value = if value <= 0.0031308 {
            value * 12.92
        } else {
            1.055 * value.powf(1.0 / 2.4) - 0.055
        };
        (value.clamp(0.0, 1.0) * 255.0).round() as u8
    };
    format!(
        "#{:02X}{:02X}{:02X}",
        encode(red),
        encode(green),
        encode(blue)
    )
}

fn hue_color_from_srgb_hex(value: &str) -> Result<HueColor, ApplicationError> {
    let value = value.strip_prefix('#').ok_or_else(|| {
        ApplicationError::Validation("light.color must use the #RRGGBB sRGB format".into())
    })?;
    if value.len() != 6 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(ApplicationError::Validation(
            "light.color must use the #RRGGBB sRGB format".into(),
        ));
    }
    let channel =
        |offset| u8::from_str_radix(&value[offset..offset + 2], 16).unwrap() as f64 / 255.0;
    let linear = |value: f64| {
        if value <= 0.04045 {
            value / 12.92
        } else {
            ((value + 0.055) / 1.055).powf(2.4)
        }
    };
    let red = linear(channel(0));
    let green = linear(channel(2));
    let blue = linear(channel(4));
    let x = 0.4124 * red + 0.3576 * green + 0.1805 * blue;
    let y = 0.2126 * red + 0.7152 * green + 0.0722 * blue;
    let z = 0.0193 * red + 0.1192 * green + 0.9505 * blue;
    let sum = x + y + z;
    if sum <= f64::EPSILON {
        return Err(ApplicationError::Validation(
            "light.color cannot be black because Hue requires chromaticity coordinates".into(),
        ));
    }
    Ok(HueColor {
        x: (x / sum).clamp(0.0, 1.0),
        y: (y / sum).clamp(0.0, 1.0),
    })
}

/// Pont vers un client réel ou un faux. Il ne stocke jamais la clé Hue : le
/// runtime l'enverra à un magasin de secrets lors de l'intégration réseau.
pub struct HueAdapter<C: HueBridgeClient> {
    client: Arc<C>,
    service: HomeService,
    dispatcher: Option<Arc<HueCommandDispatcher<C>>>,
    routes: Mutex<HashMap<String, EntityId>>,
    adapter_id: Mutex<Option<AdapterId>>,
    rooms: Mutex<Vec<HueRoom>>,
}

impl<C: HueBridgeClient> HueAdapter<C> {
    pub fn new(client: Arc<C>, service: HomeService) -> Self {
        Self {
            client,
            service,
            dispatcher: None,
            routes: Mutex::new(HashMap::new()),
            adapter_id: Mutex::new(None),
            rooms: Mutex::new(Vec::new()),
        }
    }
    pub fn with_dispatcher(
        client: Arc<C>,
        service: HomeService,
        dispatcher: Arc<HueCommandDispatcher<C>>,
    ) -> Self {
        Self {
            client,
            service,
            dispatcher: Some(dispatcher),
            routes: Mutex::new(HashMap::new()),
            adapter_id: Mutex::new(None),
            rooms: Mutex::new(Vec::new()),
        }
    }
    pub fn pair(&self) -> Result<String, HueError> {
        self.client.create_application_key()
    }
    pub fn synchronize(&self, now: DateTime<Utc>) -> Result<Vec<Device>, ApplicationError> {
        let inventory = self
            .client
            .inventory()
            .map_err(|error| ApplicationError::Infrastructure(error.to_string()))?;
        let adapter_id = AdapterId::new(format!("hue:{}", inventory.bridge_id))?;
        *self
            .adapter_id
            .lock()
            .map_err(|_| ApplicationError::Infrastructure("Hue adapter mutex poisoned".into()))? =
            Some(adapter_id.clone());
        let rooms = inventory.rooms.clone();
        let mut devices = inventory
            .lights
            .into_iter()
            .map(|light| {
                let mut capabilities = vec![
                    Capability::new("switch", 1)?,
                    Capability::new("light.brightness", 1)?,
                ];
                if light.color_temperature_mirek.is_some() {
                    capabilities.push(Capability::new("light.color_temperature", 1)?);
                }
                if light.color_xy.is_some() {
                    capabilities.push(Capability::new("light.color", 1)?);
                }
                let discovery = DeviceDiscovery {
                    adapter_id: adapter_id.clone(),
                    protocol_address: light.resource_id.clone(),
                    name: light.name.clone(),
                    entities: vec![DiscoveryEntity {
                        protocol_address: light.resource_id.clone(),
                        name: light.name,
                        kind: "light".into(),
                        capabilities,
                    }],
                };
                let device = self.service.register_discovery(discovery, now)?;
                let entity_id = device.entities[0].id.clone();
                self.routes
                    .lock()
                    .map_err(|_| {
                        ApplicationError::Infrastructure("Hue routing mutex poisoned".into())
                    })?
                    .insert(light.resource_id.clone(), entity_id.clone());
                if let Some(dispatcher) = &self.dispatcher {
                    dispatcher.register_light(entity_id.clone(), light.resource_id);
                }
                self.service.apply_reported_state(
                    ReportedState {
                        entity_id: entity_id.clone(),
                        key: "switch".into(),
                        value: StateValue::Bool(light.on),
                        source_at: now,
                    },
                    now,
                )?;
                if let Some(value) = light.brightness {
                    self.service.apply_reported_state(
                        ReportedState {
                            entity_id: entity_id.clone(),
                            key: "light.brightness".into(),
                            value: StateValue::Percentage(value.clamp(0.0, 100.0)),
                            source_at: now,
                        },
                        now,
                    )?;
                }
                if let Some(value) = light.color_temperature_mirek {
                    self.service.apply_reported_state(
                        ReportedState {
                            entity_id: entity_id.clone(),
                            key: "light.color_temperature".into(),
                            value: StateValue::Text(value.to_string()),
                            source_at: now,
                        },
                        now,
                    )?;
                }
                if let Some(value) = light.color_xy {
                    self.service.apply_reported_state(
                        ReportedState {
                            entity_id,
                            key: "light.color".into(),
                            value: StateValue::Text(srgb_hex_from_hue_color(value)),
                            source_at: now,
                        },
                        now,
                    )?;
                }
                Ok(device)
            })
            .collect::<Result<Vec<_>, ApplicationError>>()?;
        for sensor in inventory.sensors {
            let discovery = DeviceDiscovery {
                adapter_id: adapter_id.clone(),
                protocol_address: format!("sensor:{}", sensor.resource_id),
                name: sensor.name.clone(),
                entities: vec![DiscoveryEntity {
                    protocol_address: sensor.resource_id.clone(),
                    name: sensor.name,
                    kind: "sensor".into(),
                    capabilities: vec![Capability::new(sensor.key.clone(), 1)?],
                }],
            };
            let device = self.service.register_discovery(discovery, now)?;
            let entity_id = device.entities[0].id.clone();
            self.routes
                .lock()
                .map_err(|_| ApplicationError::Infrastructure("Hue routing mutex poisoned".into()))?
                .insert(sensor.resource_id, entity_id.clone());
            self.service.apply_reported_state(
                ReportedState {
                    entity_id,
                    key: sensor.key,
                    value: sensor.value,
                    source_at: now,
                },
                now,
            )?;
            devices.push(device);
        }
        *self
            .rooms
            .lock()
            .map_err(|_| ApplicationError::Infrastructure("Hue rooms mutex poisoned".into()))? =
            rooms;
        self.service.update_adapter_health(AdapterHealth {
            adapter_id,
            status: AdapterStatus::Available,
            detail: None,
            observed_at: now,
        })?;
        Ok(devices)
    }

    pub fn apply_light_update(
        &self,
        update: HueLightStateUpdate,
        received_at: DateTime<Utc>,
    ) -> Result<(), ApplicationError> {
        let entity_id = self
            .routes
            .lock()
            .map_err(|_| ApplicationError::Infrastructure("Hue routing mutex poisoned".into()))?
            .get(&update.resource_id)
            .cloned()
            .ok_or_else(|| ApplicationError::Infrastructure("unknown Hue light event".into()))?;
        let source_at = update.source_at.unwrap_or(received_at);
        if let Some(on) = update.on {
            self.service.apply_reported_state(
                ReportedState {
                    entity_id: entity_id.clone(),
                    key: "switch".into(),
                    value: StateValue::Bool(on),
                    source_at,
                },
                received_at,
            )?;
        }
        if let Some(brightness) = update.brightness {
            self.service.apply_reported_state(
                ReportedState {
                    entity_id: entity_id.clone(),
                    key: "light.brightness".into(),
                    value: StateValue::Percentage(brightness),
                    source_at,
                },
                received_at,
            )?;
        }
        if let Some(mirek) = update.color_temperature_mirek {
            self.service.apply_reported_state(
                ReportedState {
                    entity_id: entity_id.clone(),
                    key: "light.color_temperature".into(),
                    value: StateValue::Text(mirek.to_string()),
                    source_at,
                },
                received_at,
            )?;
        }
        if let Some(color) = update.color_xy {
            self.service.apply_reported_state(
                ReportedState {
                    entity_id,
                    key: "light.color".into(),
                    value: StateValue::Text(srgb_hex_from_hue_color(color)),
                    source_at,
                },
                received_at,
            )?;
        }
        Ok(())
    }

    pub fn apply_sensor_update(
        &self,
        update: HueSensorStateUpdate,
        received_at: DateTime<Utc>,
    ) -> Result<(), ApplicationError> {
        let entity_id = self
            .routes
            .lock()
            .map_err(|_| ApplicationError::Infrastructure("Hue routing mutex poisoned".into()))?
            .get(&update.resource_id)
            .cloned()
            .ok_or_else(|| ApplicationError::Infrastructure("unknown Hue sensor event".into()))?;
        self.service.apply_reported_state(
            ReportedState {
                entity_id,
                key: update.key,
                value: update.value,
                source_at: update.source_at.unwrap_or(received_at),
            },
            received_at,
        )?;
        Ok(())
    }

    /// Le dernier état rapporté reste lisible, mais aucune nouvelle commande
    /// n'est admise tant que l'inventaire et le flux n'ont pas été restaurés.
    pub fn mark_event_stream_disconnected(
        &self,
        now: DateTime<Utc>,
    ) -> Result<(), ApplicationError> {
        let adapter_id = self
            .adapter_id
            .lock()
            .map_err(|_| ApplicationError::Infrastructure("Hue adapter mutex poisoned".into()))?
            .clone();
        let Some(adapter_id) = adapter_id else {
            return Ok(());
        };
        self.service
            .set_adapter_devices_status(&adapter_id, DeviceStatus::Unavailable, now)?;
        self.service.update_adapter_health(AdapterHealth {
            adapter_id,
            status: AdapterStatus::Degraded,
            detail: Some("Hue event stream disconnected; retrying".into()),
            observed_at: now,
        })
    }

    /// Suggestions éphémères, résolues vers des entités Robine stables. Une
    /// importation explicite par le client reste nécessaire pour créer une Area.
    pub fn room_suggestions(&self) -> Result<Vec<HueRoomSuggestion>, ApplicationError> {
        let routes = self
            .routes
            .lock()
            .map_err(|_| ApplicationError::Infrastructure("Hue routing mutex poisoned".into()))?;
        let mut suggestions = self
            .rooms
            .lock()
            .map_err(|_| ApplicationError::Infrastructure("Hue rooms mutex poisoned".into()))?
            .iter()
            .filter_map(|room| {
                let mut entity_ids = room
                    .light_resource_ids
                    .iter()
                    .filter_map(|resource_id| routes.get(resource_id).cloned())
                    .collect::<Vec<_>>();
                entity_ids.sort_by_key(ToString::to_string);
                entity_ids.dedup();
                (!entity_ids.is_empty()).then_some(HueRoomSuggestion {
                    name: room.name.clone(),
                    entity_ids,
                })
            })
            .collect::<Vec<_>>();
        suggestions.sort_by(|left, right| left.name.cmp(&right.name));
        suggestions.dedup();
        Ok(suggestions)
    }
}

impl HueAdapter<HueHttpBridgeClient> {
    /// À appeler depuis la tâche supervisée du runtime après une synchronisation
    /// réussie. Un retour est une rupture de flux, à traiter par le backoff du
    /// superviseur avant une nouvelle synchronisation.
    pub fn listen_for_events(&self) -> Result<(), ApplicationError> {
        self.client
            .listen_events(|updates| {
                let received_at = Utc::now();
                for update in updates {
                    match update {
                        HueStateUpdate::Light(update) => {
                            self.apply_light_update(update, received_at)
                        }
                        HueStateUpdate::Sensor(update) => {
                            self.apply_sensor_update(update, received_at)
                        }
                    }
                    .map_err(|error| HueError::Transport(error.to_string()))?;
                }
                Ok(())
            })
            .map_err(|error| ApplicationError::Infrastructure(error.to_string()))
    }
}

/// Politique locale d'émission pour un bridge Hue. Elle ne connaît ni entité
/// ni protocole extérieur : les appels API restent des commandes canoniques.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HueCommandPolicy {
    pub minimum_light_interval: Duration,
    pub coalesce_window: Duration,
    pub queue_deadline: Duration,
}

impl Default for HueCommandPolicy {
    fn default() -> Self {
        Self {
            // Maximum conservateur de 10 commandes/s pour tout le bridge.
            minimum_light_interval: Duration::from_millis(100),
            // Absorbe les curseurs et automatisations qui visent exactement
            // la même propriété sans rendre l'ancienne demande invisible.
            coalesce_window: Duration::from_millis(20),
            queue_deadline: Duration::from_secs(5),
        }
    }
}

impl HueCommandPolicy {
    fn validate(self) -> Result<Self, ApplicationError> {
        if self.minimum_light_interval.is_zero()
            || self.coalesce_window.is_zero()
            || self.queue_deadline.is_zero()
        {
            return Err(ApplicationError::Validation(
                "Hue command policy durations must be positive".into(),
            ));
        }
        Ok(self)
    }
}

#[derive(Debug)]
struct HueRateGate {
    next_light_at: Instant,
}

impl HueRateGate {
    fn new(now: Instant) -> Self {
        Self { next_light_at: now }
    }

    fn reserve(&mut self, now: Instant, policy: HueCommandPolicy) -> Result<Instant, String> {
        let scheduled = self.next_light_at.max(now);
        if scheduled.duration_since(now) > policy.queue_deadline {
            return Err("Hue command queue deadline exceeded".into());
        }
        self.next_light_at = scheduled + policy.minimum_light_interval;
        Ok(scheduled)
    }
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
struct HueDispatchKey {
    resource_id: String,
    property: String,
}

#[derive(Clone, Debug)]
struct PendingHueValue {
    revision: u64,
    value: HueValue,
}

/// Le dispatcher est séparé de l'adaptateur pour pouvoir être enregistré au
/// démarrage du runtime. La table est peuplée par la synchronisation Hue.
pub struct HueCommandDispatcher<C: HueBridgeClient> {
    client: Option<Arc<C>>,
    routes: Mutex<HashMap<EntityId, String>>,
    policy: HueCommandPolicy,
    rate_gate: Mutex<HueRateGate>,
    pending: Mutex<HashMap<HueDispatchKey, PendingHueValue>>,
    revision: AtomicU64,
}

impl<C: HueBridgeClient> HueCommandDispatcher<C> {
    pub fn new(client: Arc<C>) -> Self {
        Self::with_policy(client, HueCommandPolicy::default())
            .expect("default Hue command policy is valid")
    }

    pub fn with_policy(client: Arc<C>, policy: HueCommandPolicy) -> Result<Self, ApplicationError> {
        let policy = policy.validate()?;
        Ok(Self {
            client: Some(client),
            routes: Mutex::new(HashMap::new()),
            policy,
            rate_gate: Mutex::new(HueRateGate::new(Instant::now())),
            pending: Mutex::new(HashMap::new()),
            revision: AtomicU64::new(0),
        })
    }
    pub fn register_light(&self, entity_id: EntityId, hue_resource_id: String) {
        self.routes
            .lock()
            .expect("hue route mutex poisoned")
            .insert(entity_id, hue_resource_id);
    }
}

impl<C: HueBridgeClient> CommandDispatcher for HueCommandDispatcher<C> {
    fn dispatch(&self, command: Command) -> Result<(), ApplicationError> {
        let resource_id = self
            .routes
            .lock()
            .map_err(|_| ApplicationError::Infrastructure("Hue routing mutex poisoned".into()))?
            .get(&command.entity_id)
            .cloned()
            .ok_or_else(|| ApplicationError::Infrastructure("no Hue route for entity".into()))?;
        let value = match (command.key.as_str(), command.value) {
            ("switch", StateValue::Bool(value)) => HueValue::On(value),
            ("light.brightness", StateValue::Percentage(value)) => {
                HueValue::Brightness(value.clamp(0.0, 100.0))
            }
            ("light.color_temperature", StateValue::Text(value)) => {
                HueValue::ColorTemperature(value.parse().map_err(|_| {
                    ApplicationError::Validation(
                        "light.color_temperature must be a Hue mirek value".into(),
                    )
                })?)
            }
            ("light.color", StateValue::Text(value)) => {
                HueValue::Color(hue_color_from_srgb_hex(&value)?)
            }
            _ => {
                return Err(ApplicationError::Infrastructure(
                    HueError::UnsupportedCommand.to_string(),
                ));
            }
        };
        let key = HueDispatchKey {
            resource_id: resource_id.clone(),
            property: command.key,
        };
        let revision = self.stage_pending(key.clone(), value)?;

        // Une courte fenêtre absorbe les demandes concurrentes de même clé.
        // Une demande remplacée termine explicitement en erreur : le coeur la
        // persiste comme échec au lieu de la présenter comme transportée.
        std::thread::sleep(self.policy.coalesce_window);
        let _value = self.current_pending_value(&key, revision)?;
        let scheduled = match self
            .rate_gate
            .lock()
            .map_err(|_| ApplicationError::Infrastructure("Hue rate gate mutex poisoned".into()))?
            .reserve(Instant::now(), self.policy)
        {
            Ok(scheduled) => scheduled,
            Err(error) => {
                self.remove_pending_if_current(&key, revision);
                return Err(ApplicationError::Infrastructure(error));
            }
        };
        std::thread::sleep(scheduled.saturating_duration_since(Instant::now()));
        let value = self
            .current_pending_value(&key, revision)
            .inspect_err(|_| {
                self.remove_pending_if_current(&key, revision);
            })?;
        let result = self
            .client
            .as_ref()
            .expect("Hue dispatcher has a client")
            .set_light_state(&resource_id, value)
            .map_err(|error| ApplicationError::Infrastructure(error.to_string()));
        self.remove_pending_if_current(&key, revision);
        result
    }
}

impl<C: HueBridgeClient> HueCommandDispatcher<C> {
    fn stage_pending(&self, key: HueDispatchKey, value: HueValue) -> Result<u64, ApplicationError> {
        let revision = self
            .revision
            .fetch_add(1, Ordering::Relaxed)
            .saturating_add(1);
        self.pending
            .lock()
            .map_err(|_| {
                ApplicationError::Infrastructure("Hue pending queue mutex poisoned".into())
            })?
            .insert(key, PendingHueValue { revision, value });
        Ok(revision)
    }

    fn current_pending_value(
        &self,
        key: &HueDispatchKey,
        revision: u64,
    ) -> Result<HueValue, ApplicationError> {
        let pending = self.pending.lock().map_err(|_| {
            ApplicationError::Infrastructure("Hue pending queue mutex poisoned".into())
        })?;
        match pending.get(key) {
            Some(value) if value.revision == revision => Ok(value.value.clone()),
            Some(_) => Err(ApplicationError::Infrastructure(
                "Hue command superseded by a newer update for the same property".into(),
            )),
            None => Err(ApplicationError::Infrastructure(
                "Hue command queue entry disappeared before dispatch".into(),
            )),
        }
    }

    fn remove_pending_if_current(&self, key: &HueDispatchKey, revision: u64) {
        if let Ok(mut pending) = self.pending.lock()
            && pending
                .get(key)
                .is_some_and(|value| value.revision == revision)
        {
            pending.remove(key);
        }
    }
}

/// Faux déterministe employé en CI ; aucune requête réseau ne lui est associée.
#[derive(Default)]
pub struct FakeHueBridge {
    pub button_pressed: Mutex<bool>,
    pub inventory: Mutex<HueInventory>,
    pub commands: Mutex<Vec<(String, HueValue)>>,
}

impl FakeHueBridge {
    pub fn with_inventory(inventory: HueInventory) -> Self {
        Self {
            inventory: Mutex::new(inventory),
            ..Self::default()
        }
    }
}

impl HueBridgeClient for FakeHueBridge {
    fn create_application_key(&self) -> Result<String, HueError> {
        if *self.button_pressed.lock().expect("fake hue mutex poisoned") {
            Ok("fake-hue-application-key".into())
        } else {
            Err(HueError::LinkButtonNotPressed)
        }
    }
    fn inventory(&self) -> Result<HueInventory, HueError> {
        Ok(self
            .inventory
            .lock()
            .expect("fake hue mutex poisoned")
            .clone())
    }
    fn set_light_state(&self, resource_id: &str, value: HueValue) -> Result<(), HueError> {
        self.commands
            .lock()
            .expect("fake hue mutex poisoned")
            .push((resource_id.into(), value));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use robine_application::HomeRepository;
    use robine_store_sqlite::SqliteStore;

    #[test]
    fn light_rate_gate_is_bounded_and_rejects_a_queue_past_its_deadline() {
        let policy = HueCommandPolicy::default();
        let now = Instant::now();
        let mut gate = HueRateGate::new(now);

        assert_eq!(gate.reserve(now, policy).unwrap(), now);
        assert_eq!(
            gate.reserve(now, policy).unwrap(),
            now + Duration::from_millis(100)
        );
        for _ in 0..49 {
            gate.reserve(now, policy).unwrap();
        }
        assert!(gate.reserve(now, policy).is_err());
    }

    #[test]
    fn newer_update_of_the_same_hue_property_supersedes_the_pending_one() {
        let bridge = Arc::new(FakeHueBridge::default());
        let policy = HueCommandPolicy {
            minimum_light_interval: Duration::from_millis(1),
            coalesce_window: Duration::from_millis(40),
            queue_deadline: Duration::from_secs(1),
        };
        let dispatcher = HueCommandDispatcher::with_policy(bridge, policy).unwrap();
        let key = HueDispatchKey {
            resource_id: "light-a".into(),
            property: "light.brightness".into(),
        };
        let first = dispatcher
            .stage_pending(key.clone(), HueValue::Brightness(20.0))
            .unwrap();
        let second = dispatcher
            .stage_pending(key.clone(), HueValue::Brightness(80.0))
            .unwrap();

        assert!(
            matches!(dispatcher.current_pending_value(&key, first), Err(ApplicationError::Infrastructure(message)) if message.contains("superseded"))
        );
        assert_eq!(
            dispatcher.current_pending_value(&key, second).unwrap(),
            HueValue::Brightness(80.0)
        );
    }

    #[test]
    fn reconnect_backoff_is_exponential_capped_and_jittered() {
        let policy = HueReconnectBackoff::default();

        assert_eq!(policy.delay(1, 0), Duration::from_secs(1));
        assert_eq!(policy.delay(2, 0), Duration::from_secs(2));
        assert_eq!(policy.delay(3, 0), Duration::from_secs(4));
        assert_eq!(policy.delay(99, 0), Duration::from_secs(60));
        assert_eq!(policy.delay(2, u16::MAX), Duration::from_millis(2_400));
        assert_eq!(policy.delay(99, u16::MAX), Duration::from_secs(60));
    }

    #[test]
    fn synchronization_keeps_identity_and_commands_the_mapped_hue_light() {
        let bridge = Arc::new(FakeHueBridge::with_inventory(HueInventory {
            bridge_id: "bridge-a".into(),
            lights: vec![HueLight {
                resource_id: "light-a".into(),
                name: "Lampe du salon".into(),
                on: true,
                brightness: Some(42.0),
                color_temperature_mirek: Some(250),
                color_xy: Some(HueColor {
                    x: 0.3127,
                    y: 0.3290,
                }),
            }],
            sensors: vec![HueSensor {
                resource_id: "motion-a".into(),
                name: "Mouvement entrée".into(),
                key: "sensor.occupancy".into(),
                value: StateValue::Bool(false),
            }],
            rooms: vec![HueRoom {
                name: "Salon Hue".into(),
                light_resource_ids: vec!["light-a".into()],
            }],
        }));
        let store = Arc::new(SqliteStore::open_in_memory().unwrap());
        let dispatcher = Arc::new(HueCommandDispatcher::new(bridge.clone()));
        let service = HomeService::new(store.clone(), store.clone(), dispatcher.clone());
        let adapter = HueAdapter::with_dispatcher(bridge.clone(), service.clone(), dispatcher);
        let now = Utc::now();

        let first = adapter.synchronize(now).unwrap();
        let second = adapter.synchronize(now).unwrap();
        assert_eq!(first[0].id, second[0].id);
        assert_eq!(first[0].entities[0].id, second[0].entities[0].id);
        assert!(
            first[0].entities[0]
                .capabilities
                .iter()
                .any(|capability| capability.key == "light.color_temperature")
        );
        let motion = first
            .iter()
            .flat_map(|device| device.entities.iter())
            .find(|entity| entity.name == "Mouvement entrée")
            .expect("Hue sensor is discovered");
        assert!(motion.supports("sensor.occupancy"));
        assert_eq!(
            service
                .entity_detail(&motion.id)
                .unwrap()
                .expect("discovered Hue sensor has an entity projection")
                .state
                .iter()
                .find(|state| state.key == "sensor.occupancy")
                .map(|state| state.value.clone()),
            Some(StateValue::Bool(false))
        );
        assert!(
            first[0].entities[0]
                .capabilities
                .iter()
                .any(|capability| capability.key == "light.color")
        );
        assert!(
            store
                .list_adapter_health()
                .unwrap()
                .iter()
                .any(|health| health.status == AdapterStatus::Available)
        );
        assert_eq!(
            adapter.room_suggestions().unwrap(),
            vec![HueRoomSuggestion {
                name: "Salon Hue".into(),
                entity_ids: vec![first[0].entities[0].id.clone()],
            }]
        );

        let command = service
            .request_command(
                first[0].entities[0].id.clone(),
                "switch".into(),
                StateValue::Bool(false),
                "toggle-1".into(),
                now,
            )
            .unwrap();
        assert_eq!(command.idempotency_key, "toggle-1");
        assert_eq!(
            *bridge.commands.lock().unwrap(),
            vec![("light-a".into(), HueValue::On(false))]
        );
        service
            .request_command(
                first[0].entities[0].id.clone(),
                "light.color".into(),
                StateValue::Text("#FF0000".into()),
                "color-1".into(),
                now,
            )
            .unwrap();
        let commands = bridge.commands.lock().unwrap();
        assert!(matches!(
            commands.last(),
            Some((resource, HueValue::Color(HueColor { x, y })))
                if resource == "light-a" && (*x - 0.6401).abs() < 0.001 && (*y - 0.3300).abs() < 0.001
        ));
        drop(commands);
        adapter.mark_event_stream_disconnected(now).unwrap();
        assert_eq!(
            service.list_devices().unwrap()[0].status,
            DeviceStatus::Unavailable
        );
        assert!(matches!(
            service.request_command(
                first[0].entities[0].id.clone(),
                "switch".into(),
                StateValue::Bool(true),
                "unavailable-1".into(),
                now,
            ),
            Err(ApplicationError::Validation(_))
        ));
        assert!(
            store
                .list_adapter_health()
                .unwrap()
                .iter()
                .any(|health| health.adapter_id.0 == "hue:bridge-a"
                    && health.status == AdapterStatus::Degraded)
        );
        adapter
            .synchronize(now + chrono::Duration::seconds(1))
            .unwrap();
        assert_eq!(
            service.list_devices().unwrap()[0].status,
            DeviceStatus::Available
        );
        let command_id = command.id.clone();
        let reported_at = now + chrono::Duration::seconds(2);
        adapter
            .apply_light_update(
                HueLightStateUpdate {
                    resource_id: "light-a".into(),
                    on: Some(false),
                    brightness: None,
                    color_temperature_mirek: None,
                    color_xy: None,
                    source_at: Some(reported_at),
                },
                reported_at,
            )
            .unwrap();
        assert!(
            store
                .events_after(0, 100)
                .unwrap()
                .iter()
                .any(|event| matches!(event.data, EventData::StateReported { .. }))
        );
        assert!(store.events_after(0, 100).unwrap().iter().any(
            |event| matches!(&event.data, EventData::CommandConfirmed { command } if command.id == command_id)
        ));
    }

    #[test]
    fn parses_partial_light_updates_from_hue_v2_eventstream() {
        let updates = parse_v2_event(
            r#"[{"type":"update","data":[{"type":"light","id":"light-a","on":{"on":false}},{"type":"motion","id":"motion-a","motion":{"motion":true}},{"type":"temperature","id":"temperature-a","temperature":{"temperature":19.5}},{"type":"device_power","id":"power-a","power_state":{"battery_level":82}},{"type":"contact","id":"contact-a","contact_report":{"state":"contact"}},{"type":"light","id":"light-b","dimming":{"brightness":17.5}},{"type":"light","id":"light-c","color_temperature":{"mirek":250}},{"type":"light","id":"light-d","color":{"xy":{"x":0.3127,"y":0.329}}},{"type":"device","id":"ignored"}]}]"#,
        )
        .unwrap();
        assert_eq!(
            updates,
            vec![
                HueStateUpdate::Light(HueLightStateUpdate {
                    resource_id: "light-a".into(),
                    on: Some(false),
                    brightness: None,
                    color_temperature_mirek: None,
                    color_xy: None,
                    source_at: None,
                }),
                HueStateUpdate::Sensor(HueSensorStateUpdate {
                    resource_id: "motion-a".into(),
                    key: "sensor.occupancy".into(),
                    value: StateValue::Bool(true),
                    source_at: None,
                }),
                HueStateUpdate::Sensor(HueSensorStateUpdate {
                    resource_id: "temperature-a".into(),
                    key: "sensor.temperature".into(),
                    value: StateValue::Text("19.5".into()),
                    source_at: None,
                }),
                HueStateUpdate::Sensor(HueSensorStateUpdate {
                    resource_id: "power-a".into(),
                    key: "sensor.battery".into(),
                    value: StateValue::Percentage(82.0),
                    source_at: None,
                }),
                HueStateUpdate::Sensor(HueSensorStateUpdate {
                    resource_id: "contact-a".into(),
                    key: "sensor.binary".into(),
                    value: StateValue::Bool(true),
                    source_at: None,
                }),
                HueStateUpdate::Light(HueLightStateUpdate {
                    resource_id: "light-b".into(),
                    on: None,
                    brightness: Some(17.5),
                    color_temperature_mirek: None,
                    color_xy: None,
                    source_at: None,
                }),
                HueStateUpdate::Light(HueLightStateUpdate {
                    resource_id: "light-c".into(),
                    on: None,
                    brightness: None,
                    color_temperature_mirek: Some(250),
                    color_xy: None,
                    source_at: None,
                }),
                HueStateUpdate::Light(HueLightStateUpdate {
                    resource_id: "light-d".into(),
                    on: None,
                    brightness: None,
                    color_temperature_mirek: None,
                    color_xy: Some(HueColor {
                        x: 0.3127,
                        y: 0.329
                    }),
                    source_at: None,
                }),
            ]
        );
    }

    #[test]
    fn room_and_zone_resources_become_light_only_import_suggestions() {
        let room = hue_room_from_resource(&json!({
            "metadata": { "name": "Salon" },
            "children": [
                { "rid": "light-a", "rtype": "light" },
                { "rid": "sensor-a", "rtype": "temperature" },
                { "rid": "light-a", "rtype": "light" }
            ]
        }))
        .unwrap();
        let zone = hue_room_from_resource(&json!({
            "metadata": { "name": "Toute la maison" },
            "children": [
                { "rid": "light-b", "rtype": "light" }
            ]
        }))
        .unwrap();
        assert_eq!(room.name, "Salon");
        assert_eq!(room.light_resource_ids, ["light-a"]);
        assert_eq!(zone.name, "Toute la maison");
        assert_eq!(zone.light_resource_ids, ["light-b"]);
        assert!(
            hue_room_from_resource(&json!({
                "metadata": { "name": "Capteurs seulement" },
                "children": [{ "rid": "sensor-a", "rtype": "temperature" }]
            }))
            .is_none()
        );
    }

    #[test]
    fn rejects_ambiguous_bridge_authorities_before_tls_setup() {
        for authority in [
            "https://hue.local",
            "hue.local/path",
            "user@hue.local",
            "hue.local?x=1",
            "hue local",
        ] {
            assert!(matches!(
                HueHttpBridgeClient::with_pinned_certificate(authority, b"not-a-certificate", None),
                Err(HueError::Transport(message)) if message == "invalid bridge authority"
            ));
        }
    }

    #[test]
    fn color_uses_a_strict_srgb_contract_at_the_hue_boundary() {
        let red = hue_color_from_srgb_hex("#FF0000").unwrap();
        assert!((red.x - 0.6401).abs() < 0.001);
        assert!((red.y - 0.3300).abs() < 0.001);
        assert_eq!(srgb_hex_from_hue_color(red), "#FF0000");
        assert!(hue_color_from_srgb_hex("FF0000").is_err());
        assert!(hue_color_from_srgb_hex("#FF00GG").is_err());
        assert!(hue_color_from_srgb_hex("#000000").is_err());
    }
}
