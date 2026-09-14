use super::{error, proto as p, Instance, Module, Result, TopLevelModule};
use prost::Message;
use serde::{de::DeserializeOwned, Serialize};
use serde_json::{Map, Value};
use std::collections::BTreeMap;

type Properties = BTreeMap<String, String>;
fn put<T: Serialize>(props: &mut Properties, key: &str, value: &T) -> Result<()> {
    props.insert(key.into(), serde_json::to_string(value).map_err(error)?);
    Ok(())
}
fn get<T: DeserializeOwned + Default>(props: &Properties, key: &str) -> Result<T> {
    props
        .get(key)
        .map(|s| serde_json::from_str(s).map_err(error))
        .transpose()
        .map(|v| v.unwrap_or_default())
}
fn parameter(value: &Value) -> Option<p::ParameterValue> {
    value.as_f64().map(|v| p::ParameterValue {
        value: Some(p::parameter_value::Value::PrefixedValue(p::PrefixedValue {
            double_value: v,
            prefix: 0,
        })),
    })
}
fn number(value: &p::ParameterValue) -> Option<Value> {
    match &value.value {
        Some(p::parameter_value::Value::PrefixedValue(v)) => Some(Value::from(v.double_value)),
        _ => None,
    }
}
fn terminal(text: &str) -> p::TerminalReference {
    let (instance, port) = text.split_once(',').unwrap_or(("", text));
    p::TerminalReference {
        instance_name: instance.trim().into(),
        terminal_name: port.trim().into(),
    }
}
fn reference(t: &p::TerminalReference) -> String {
    if t.instance_name.is_empty() {
        t.terminal_name.clone()
    } else {
        format!("{},{}", t.instance_name, t.terminal_name)
    }
}
fn connection(name: String, source: &str, target: &str) -> p::Connection {
    p::Connection {
        name,
        source: Some(terminal(source)),
        target: Some(terminal(target)),
        ..Default::default()
    }
}
impl Instance {
    fn to_proto_ref(&self, name: &str) -> Result<p::ModuleReference> {
        let mut properties = Properties::new();
        put(&mut properties, "__array__", &self.array)?;
        put(&mut properties, "__info__", &self.info)?;
        let mut parameter_overrides = BTreeMap::new();
        for (key, value) in &self.settings {
            if let Some(p) = parameter(value) {
                parameter_overrides.insert(key.clone(), p);
            }
            // Keep exact JSON types/integers alongside the numeric protobuf representation.
            put(&mut properties, &format!("__setting__{key}"), value)?;
        }
        Ok(p::ModuleReference {
            name: name.into(),
            module_name: self.component.clone(),
            parameter_overrides,
            properties,
            ..Default::default()
        })
    }
    fn from_proto_ref(r: &p::ModuleReference) -> Result<Self> {
        let mut settings: Map<String, Value> = r
            .parameter_overrides
            .iter()
            .filter_map(|(k, v)| number(v).map(|v| (k.clone(), v)))
            .collect();
        for (k, v) in &r.properties {
            if let Some(k) = k.strip_prefix("__setting__") {
                settings.insert(k.into(), serde_json::from_str(v).map_err(error)?);
            }
        }
        Ok(Self {
            component: r.module_name.clone(),
            settings,
            array: get(&r.properties, "__array__")?,
            info: get(&r.properties, "__info__")?,
        })
    }
}
impl Module {
    fn to_proto_module(&self, name: &str) -> Result<p::Module> {
        let mut properties = Properties::new();
        put(&mut properties, "__name__", &self.name)?;
        put(&mut properties, "__placements__", &self.placements)?;
        put(&mut properties, "__routes__", &self.routes)?;
        put(&mut properties, "__info__", &self.info)?;
        let mut parameters = Vec::new();
        for (i, (name, value)) in self.settings.iter().enumerate() {
            let mut properties = Properties::new();
            put(&mut properties, "__json_value__", value)?;
            parameters.push(p::Parameter {
                uid: i as u32,
                name: name.clone(),
                default_value: parameter(value),
                properties,
                ..Default::default()
            });
        }
        let module_references = self
            .instances
            .iter()
            .map(|(name, i)| i.to_proto_ref(name))
            .collect::<Result<_>>()?;
        let terminal = self
            .ports
            .keys()
            .enumerate()
            .map(|(i, name)| p::Terminal {
                uid: i as u32,
                name: name.clone(),
                ..Default::default()
            })
            .collect();
        let mut connections: Vec<_> = self
            .connections
            .iter()
            .enumerate()
            .map(|(i, (src, tgt))| connection(format!("conn_{i}"), src, tgt))
            .collect();
        for (name, target) in &self.ports {
            connections.push(connection(format!("port_{name}"), name, target));
        }
        let mut buses = Vec::new();
        for (i, members) in self.nets.iter().enumerate() {
            let mut properties = Properties::new();
            // Preserve empty/single-member nets, which cannot be expressed as connection pairs.
            put(&mut properties, "__members__", members)?;
            buses.push(p::Bus {
                name: format!("net_{i}"),
                connections: members
                    .windows(2)
                    .enumerate()
                    .map(|(j, pair)| connection(format!("net{i}_c{j}"), &pair[0], &pair[1]))
                    .collect(),
                properties,
                ..Default::default()
            });
        }
        Ok(p::Module {
            name: name.into(),
            terminal,
            parameters,
            module_references,
            connections,
            buses,
            properties,
            ..Default::default()
        })
    }
    fn from_proto_module(pm: &p::Module) -> Result<Self> {
        let mut settings = Map::new();
        for p in &pm.parameters {
            if let Some(v) = p.properties.get("__json_value__") {
                settings.insert(p.name.clone(), serde_json::from_str(v).map_err(error)?);
            } else if let Some(v) = p.default_value.as_ref().and_then(number) {
                settings.insert(p.name.clone(), v);
            }
        }
        let mut module = Self {
            name: if pm.properties.contains_key("__name__") {
                get(&pm.properties, "__name__")?
            } else {
                Some(pm.name.clone())
            },
            settings,
            info: get(&pm.properties, "__info__")?,
            placements: get(&pm.properties, "__placements__")?,
            routes: get(&pm.properties, "__routes__")?,
            instances: pm
                .module_references
                .iter()
                .map(|r| Ok((r.name.clone(), Instance::from_proto_ref(r)?)))
                .collect::<Result<_>>()?,
            ..Default::default()
        };
        for c in &pm.connections {
            if let (Some(src), Some(tgt)) = (&c.source, &c.target) {
                if c.name.starts_with("port_") {
                    module
                        .ports
                        .insert(src.terminal_name.clone(), reference(tgt));
                } else {
                    module.connections.insert(reference(src), reference(tgt));
                }
            }
        }
        for t in &pm.terminal {
            module
                .ports
                .entry(t.name.clone())
                .or_insert_with(|| t.name.clone());
        }
        for bus in &pm.buses {
            let members = if bus.properties.contains_key("__members__") {
                get(&bus.properties, "__members__")?
            } else {
                let mut members = Vec::new();
                for c in &bus.connections {
                    for t in [&c.source, &c.target].into_iter().flatten() {
                        let r = reference(t);
                        if members.last() != Some(&r) {
                            members.push(r);
                        }
                    }
                }
                members
            };
            module.nets.push(members);
        }
        Ok(module)
    }
}
impl TopLevelModule {
    pub fn to_proto_circuit(&self) -> Result<p::Circuit> {
        Ok(p::Circuit {
            name: self.toplevel.clone().unwrap_or_default(),
            top_module: self.toplevel.clone().unwrap_or_default(),
            modules: self
                .modules
                .iter()
                .map(|(name, m)| m.to_proto_module(name))
                .collect::<Result<_>>()?,
            ..Default::default()
        })
    }
    pub fn from_proto_circuit(c: &p::Circuit) -> Result<Self> {
        Ok(Self {
            modules: c
                .modules
                .iter()
                .map(|m| Ok((m.name.clone(), Module::from_proto_module(m)?)))
                .collect::<Result<_>>()?,
            toplevel: if c.top_module.is_empty() {
                None
            } else {
                Some(c.top_module.clone())
            },
        })
    }
    pub fn to_proto(&self) -> Result<Vec<u8>> {
        Ok(self.to_proto_circuit()?.encode_to_vec())
    }
    pub fn from_proto(data: &[u8]) -> Result<Self> {
        Self::from_proto_circuit(&p::Circuit::decode(data).map_err(error)?)
    }
}
