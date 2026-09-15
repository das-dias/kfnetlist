//! Python object conversion only; all schema operations live in kfnetlist-core.
use crate::{from_py_any, json_parse, json_string, to_py_dict};
use kfnetlist_core::schema::{self as core, proto};
use prost::Message;
use pyo3::prelude::*;
use pyo3::types::{PyBytes, PyDict, PyList, PyTuple, PyType};
use serde::{Deserialize, Serialize};
use serde_json::Value;

fn schema_error(e: impl std::fmt::Display) -> PyErr {
    pyo3::exceptions::PyValueError::new_err(e.to_string())
}

// Accept native schema objects nested in constructor dicts/lists without
// importing Python schema, protobuf, YAML, or pydantic implementations.
fn input_value(obj: &Bound<'_, PyAny>) -> PyResult<Value> {
    if let Ok(d) = obj.downcast::<PyDict>() {
        let mut map = serde_json::Map::new();
        for (k, v) in d {
            map.insert(k.extract()?, input_value(&v)?);
        }
        Ok(Value::Object(map))
    } else if obj.is_instance_of::<PyList>() || obj.is_instance_of::<PyTuple>() {
        Ok(Value::Array(
            obj.try_iter()?
                .map(|v| input_value(&v?))
                .collect::<PyResult<_>>()?,
        ))
    } else if obj.hasattr("to_dict")? {
        from_py_any(&obj.call_method0("to_dict")?)
    } else if obj.hasattr("__int__")?
        && !obj.is_instance_of::<pyo3::types::PyFloat>()
        && !obj.is_instance_of::<pyo3::types::PyBool>()
    {
        from_py_any(&obj.call_method0("__int__")?)
    } else {
        from_py_any(obj)
    }
}
trait Field {
    fn field(&self, py: Python<'_>) -> PyResult<PyObject>;
}
macro_rules! scalar_field {
    ($($t:ty),*) => { $(impl Field for $t {
        fn field(&self, py: Python<'_>) -> PyResult<PyObject> { Ok(to_py_dict(py, self)?.unbind()) }
    })* };
}
scalar_field!(String, i32, i64, u32, f64, Value, serde_json::Map<String, Value>);
impl<T: Field> Field for Option<T> {
    fn field(&self, py: Python<'_>) -> PyResult<PyObject> {
        match self {
            Some(v) => v.field(py),
            None => Ok(py.None()),
        }
    }
}
impl<T: Field> Field for Vec<T> {
    fn field(&self, py: Python<'_>) -> PyResult<PyObject> {
        Ok(PyList::new(
            py,
            self.iter()
                .map(|v| v.field(py))
                .collect::<PyResult<Vec<_>>>()?,
        )?
        .into_any()
        .unbind())
    }
}
macro_rules! map_field {
    ($map:ty) => {
        impl<T: Field> Field for $map {
            fn field(&self, py: Python<'_>) -> PyResult<PyObject> {
                let d = PyDict::new(py);
                for (k, v) in self {
                    d.set_item(k, v.field(py)?)?;
                }
                Ok(d.into_any().unbind())
            }
        }
    };
}
map_field!(indexmap::IndexMap<String, T>);
map_field!(std::collections::BTreeMap<String, T>);

macro_rules! schema_class {
    ($name:ident, $core:ty, [$($field:ident),*], {$($extra:tt)*}) => {
        #[pyclass(module = "kfnetlist._native", frozen)]
        #[derive(Clone, Debug, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(pub $core);
        impl Field for $core {
            fn field(&self, py: Python<'_>) -> PyResult<PyObject> {
                Ok(Py::new(py, $name(self.clone()))?.into_any())
            }
        }
        #[pymethods]
        impl $name {
            #[new]
            #[pyo3(signature = (**kwargs))]
            fn new(kwargs: Option<&Bound<'_, PyDict>>) -> PyResult<Self> {
                let value = kwargs.map(|v| input_value(v.as_any())).transpose()?.unwrap_or_else(|| serde_json::json!({}));
                serde_json::from_value(value).map(Self).map_err(schema_error)
            }
            $(#[getter]
            fn $field(&self, py: Python<'_>) -> PyResult<PyObject> { self.0.$field.field(py) })*
            fn to_dict<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> { to_py_dict(py, &self.0) }
            #[pyo3(signature = (*, mode="python"))]
            fn model_dump<'py>(&self, py: Python<'py>, mode: &str) -> PyResult<Bound<'py, PyAny>> {
                let _ = mode;
                self.to_dict(py)
            }
            fn to_json(&self) -> PyResult<String> { json_string(&self.0) }
            fn model_dump_json(&self) -> PyResult<String> { self.to_json() }
            #[classmethod]
            fn from_dict(_cls: &Bound<'_, PyType>, obj: &Bound<'_, PyAny>) -> PyResult<Self> {
                serde_json::from_value(input_value(obj)?).map(Self).map_err(schema_error)
            }
            #[classmethod]
            fn model_validate(cls: &Bound<'_, PyType>, obj: &Bound<'_, PyAny>) -> PyResult<Self> { Self::from_dict(cls, obj) }
            #[classmethod]
            fn from_json(_cls: &Bound<'_, PyType>, data: &str) -> PyResult<Self> {
                serde_json::from_value(json_parse(data)?).map(Self).map_err(schema_error)
            }
            #[classmethod]
            fn model_validate_json(cls: &Bound<'_, PyType>, data: &str) -> PyResult<Self> { Self::from_json(cls, data) }
            #[classmethod]
            fn __get_pydantic_core_schema__(cls: &Bound<'_, PyType>, _source_type: &Bound<'_, PyAny>, _handler: &Bound<'_, PyAny>) -> PyResult<PyObject> {
                crate::pydantic_core_schema(cls)
            }
            fn __eq__(&self, other: &Self) -> bool { self.0 == other.0 }
            fn __repr__(&self) -> PyResult<String> { Ok(format!("{}({})", stringify!($name), self.to_json()?)) }
            $($extra)*
        }
    };
}
schema_class!(
    PrefixedValue,
    proto::PrefixedValue,
    [double_value, prefix],
    {
        fn to_proto<'py>(&self, py: Python<'py>) -> Bound<'py, PyBytes> {
            PyBytes::new(py, &self.0.encode_to_vec())
        }
        #[classmethod]
        fn from_proto(_cls: &Bound<'_, PyType>, data: &[u8]) -> PyResult<Self> {
            <proto::PrefixedValue>::decode(data)
                .map(Self)
                .map_err(schema_error)
        }
    }
);
schema_class!(ParameterValue, proto::ParameterValue, [], {
    fn to_proto<'py>(&self, py: Python<'py>) -> Bound<'py, PyBytes> {
        PyBytes::new(py, &self.0.encode_to_vec())
    }
    #[classmethod]
    fn from_proto(_cls: &Bound<'_, PyType>, data: &[u8]) -> PyResult<Self> {
        <proto::ParameterValue>::decode(data)
            .map(Self)
            .map_err(schema_error)
    }

    #[getter]
    fn prefixed_value(&self) -> Option<PrefixedValue> {
        match &self.0.value {
            Some(proto::parameter_value::Value::PrefixedValue(v)) => Some(PrefixedValue(*v)),
            _ => None,
        }
    }
    #[getter]
    fn model_ref(&self) -> Option<ModelReference> {
        match &self.0.value {
            Some(proto::parameter_value::Value::ModelRef(v)) => Some(ModelReference(v.clone())),
            _ => None,
        }
    }
});
schema_class!(
    Parameter,
    proto::Parameter,
    [uid, name, default_value, description, properties],
    {
        fn to_proto<'py>(&self, py: Python<'py>) -> Bound<'py, PyBytes> {
            PyBytes::new(py, &self.0.encode_to_vec())
        }
        #[classmethod]
        fn from_proto(_cls: &Bound<'_, PyType>, data: &[u8]) -> PyResult<Self> {
            <proto::Parameter>::decode(data)
                .map(Self)
                .map_err(schema_error)
        }
    }
);
schema_class!(
    ModelInterface,
    proto::ModelInterface,
    [name, function_name, parameters, properties],
    {
        fn to_proto<'py>(&self, py: Python<'py>) -> Bound<'py, PyBytes> {
            PyBytes::new(py, &self.0.encode_to_vec())
        }
        #[classmethod]
        fn from_proto(_cls: &Bound<'_, PyType>, data: &[u8]) -> PyResult<Self> {
            <proto::ModelInterface>::decode(data)
                .map(Self)
                .map_err(schema_error)
        }
    }
);
schema_class!(
    ModelReference,
    proto::ModelReference,
    [model_interface_name, arguments],
    {
        fn to_proto<'py>(&self, py: Python<'py>) -> Bound<'py, PyBytes> {
            PyBytes::new(py, &self.0.encode_to_vec())
        }
        #[classmethod]
        fn from_proto(_cls: &Bound<'_, PyType>, data: &[u8]) -> PyResult<Self> {
            <proto::ModelReference>::decode(data)
                .map(Self)
                .map_err(schema_error)
        }
    }
);
schema_class!(
    Terminal,
    proto::Terminal,
    [
        uid,
        name,
        direction,
        domain,
        width,
        cross_section,
        properties
    ],
    {
        fn to_proto<'py>(&self, py: Python<'py>) -> Bound<'py, PyBytes> {
            PyBytes::new(py, &self.0.encode_to_vec())
        }
        #[classmethod]
        fn from_proto(_cls: &Bound<'_, PyType>, data: &[u8]) -> PyResult<Self> {
            <proto::Terminal>::decode(data)
                .map(Self)
                .map_err(schema_error)
        }
    }
);
schema_class!(
    TerminalReference,
    proto::TerminalReference,
    [instance_name, terminal_name],
    {
        fn to_proto<'py>(&self, py: Python<'py>) -> Bound<'py, PyBytes> {
            PyBytes::new(py, &self.0.encode_to_vec())
        }
        #[classmethod]
        fn from_proto(_cls: &Bound<'_, PyType>, data: &[u8]) -> PyResult<Self> {
            <proto::TerminalReference>::decode(data)
                .map(Self)
                .map_err(schema_error)
        }
    }
);
schema_class!(
    Connection,
    proto::Connection,
    [name, source, target, domain, weight, properties],
    {
        fn to_proto<'py>(&self, py: Python<'py>) -> Bound<'py, PyBytes> {
            PyBytes::new(py, &self.0.encode_to_vec())
        }
        #[classmethod]
        fn from_proto(_cls: &Bound<'_, PyType>, data: &[u8]) -> PyResult<Self> {
            <proto::Connection>::decode(data)
                .map(Self)
                .map_err(schema_error)
        }
    }
);
schema_class!(
    Bus,
    proto::Bus,
    [name, width, domain, connections, properties],
    {
        fn to_proto<'py>(&self, py: Python<'py>) -> Bound<'py, PyBytes> {
            PyBytes::new(py, &self.0.encode_to_vec())
        }
        #[classmethod]
        fn from_proto(_cls: &Bound<'_, PyType>, data: &[u8]) -> PyResult<Self> {
            <proto::Bus>::decode(data).map(Self).map_err(schema_error)
        }
    }
);
schema_class!(
    ExternalModule,
    proto::ExternalModule,
    [name, domain, terminals, parameters, properties],
    {
        fn to_proto<'py>(&self, py: Python<'py>) -> Bound<'py, PyBytes> {
            PyBytes::new(py, &self.0.encode_to_vec())
        }
        #[classmethod]
        fn from_proto(_cls: &Bound<'_, PyType>, data: &[u8]) -> PyResult<Self> {
            <proto::ExternalModule>::decode(data)
                .map(Self)
                .map_err(schema_error)
        }
    }
);
schema_class!(
    ProtoModuleReference,
    proto::ModuleReference,
    [
        name,
        module_name,
        class_name,
        parameter_values,
        parameter_overrides,
        properties
    ],
    {
        fn to_proto<'py>(&self, py: Python<'py>) -> Bound<'py, PyBytes> {
            PyBytes::new(py, &self.0.encode_to_vec())
        }
        #[classmethod]
        fn from_proto(_cls: &Bound<'_, PyType>, data: &[u8]) -> PyResult<Self> {
            <proto::ModuleReference>::decode(data)
                .map(Self)
                .map_err(schema_error)
        }
    }
);
schema_class!(
    ProtoModule,
    proto::Module,
    [
        uid,
        name,
        class_name,
        terminal,
        parameters,
        model_interfaces,
        module_references,
        connections,
        buses,
        properties
    ],
    {
        fn to_proto<'py>(&self, py: Python<'py>) -> Bound<'py, PyBytes> {
            PyBytes::new(py, &self.0.encode_to_vec())
        }
        #[classmethod]
        fn from_proto(_cls: &Bound<'_, PyType>, data: &[u8]) -> PyResult<Self> {
            <proto::Module>::decode(data)
                .map(Self)
                .map_err(schema_error)
        }
    }
);
schema_class!(
    ProtoCircuit,
    proto::Circuit,
    [name, domain, top_module, modules, ext_modules, properties],
    {
        fn to_proto<'py>(&self, py: Python<'py>) -> Bound<'py, PyBytes> {
            PyBytes::new(py, &self.0.encode_to_vec())
        }
        #[classmethod]
        fn from_proto(_cls: &Bound<'_, PyType>, data: &[u8]) -> PyResult<Self> {
            <proto::Circuit>::decode(data)
                .map(Self)
                .map_err(schema_error)
        }
    }
);
schema_class!(ArraySpec, core::ArraySpec, [na, nb], {});
schema_class!(
    Instance,
    core::Instance,
    [component, settings, array, info],
    {}
);
schema_class!(
    Module,
    core::Module,
    [
        name,
        settings,
        info,
        instances,
        placements,
        ports,
        connections,
        nets,
        routes
    ],
    {
        fn to_netlist(&self) -> PyResult<crate::netlist::Netlist> {
            self.0
                .to_netlist()
                .map(crate::netlist::Netlist)
                .map_err(schema_error)
        }
        #[classmethod]
        fn from_netlist(
            _cls: &Bound<'_, PyType>,
            name: String,
            nl: &crate::netlist::Netlist,
        ) -> Self {
            Self(core::Module::from_netlist(name, &nl.0))
        }
    }
);
schema_class!(TopLevelModule, core::TopLevelModule, [modules, toplevel], {
    fn to_proto_circuit(&self) -> PyResult<ProtoCircuit> {
        self.0
            .to_proto_circuit()
            .map(ProtoCircuit)
            .map_err(schema_error)
    }
    #[classmethod]
    fn from_proto_circuit(_cls: &Bound<'_, PyType>, circuit: &ProtoCircuit) -> PyResult<Self> {
        core::TopLevelModule::from_proto_circuit(&circuit.0)
            .map(Self)
            .map_err(schema_error)
    }
    fn to_proto<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyBytes>> {
        Ok(PyBytes::new(py, &self.0.to_proto().map_err(schema_error)?))
    }
    #[classmethod]
    fn from_proto(_cls: &Bound<'_, PyType>, data: &[u8]) -> PyResult<Self> {
        core::TopLevelModule::from_proto(data)
            .map(Self)
            .map_err(schema_error)
    }
    fn to_yaml(&self) -> PyResult<String> {
        self.0.to_yaml().map_err(schema_error)
    }
    #[classmethod]
    fn from_yaml(_cls: &Bound<'_, PyType>, text: &str) -> PyResult<Self> {
        core::TopLevelModule::from_yaml(text)
            .map(Self)
            .map_err(schema_error)
    }
    fn to_netlists<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyDict>> {
        let result = PyDict::new(py);
        for (k, v) in self.0.to_netlists().map_err(schema_error)? {
            result.set_item(k, crate::netlist::Netlist(v))?;
        }
        Ok(result)
    }
    #[classmethod]
    #[pyo3(signature = (netlists, toplevel=None))]
    fn from_netlists(
        _cls: &Bound<'_, PyType>,
        netlists: &Bound<'_, PyDict>,
        toplevel: Option<String>,
    ) -> PyResult<Self> {
        let netlists = netlists
            .iter()
            .map(|(k, v)| {
                Ok((
                    k.extract()?,
                    v.extract::<PyRef<'_, crate::netlist::Netlist>>()?.0.clone(),
                ))
            })
            .collect::<PyResult<_>>()?;
        Ok(Self(core::TopLevelModule::from_netlists(
            &netlists, toplevel,
        )))
    }
});
#[allow(clippy::upper_case_acronyms)]
#[pyclass(module = "kfnetlist._native", eq, eq_int)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TerminalDirection {
    INOUT = 0,
    INPUT = 1,
    OUTPUT = 2,
}
#[allow(clippy::upper_case_acronyms)]
#[pyclass(module = "kfnetlist._native", eq, eq_int)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SignalDomain {
    UNSPECIFIED = 0,
    ELECTRICAL = 1,
    WAVEGUIDE = 2,
}
#[allow(clippy::upper_case_acronyms)]
#[pyclass(module = "kfnetlist._native", eq, eq_int)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SIPrefix {
    UNSPECIFIED = 0,
    QUECTO = 1,
    RONTO = 2,
    YOCTO = 3,
    ZEPTO = 4,
    ATTO = 5,
    FEMTO = 6,
    PICO = 7,
    NANO = 8,
    MICRO = 9,
    MILLI = 10,
    CENTI = 11,
    DECI = 12,
    DECA = 13,
    HECTO = 14,
    KILO = 15,
    MEGA = 16,
    GIGA = 17,
    TERA = 18,
    PETA = 19,
    EXA = 20,
    ZETTA = 21,
    YOTTA = 22,
    RONNA = 23,
    QUETTA = 24,
}
#[pyfunction]
pub fn load_pic_yaml(path: std::path::PathBuf) -> PyResult<TopLevelModule> {
    core::TopLevelModule::load_pic_yaml(path)
        .map(TopLevelModule)
        .map_err(schema_error)
}
pub fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<PrefixedValue>()?;
    m.add_class::<ParameterValue>()?;
    m.add_class::<Parameter>()?;
    m.add_class::<ModelInterface>()?;
    m.add_class::<ModelReference>()?;
    m.add_class::<Terminal>()?;
    m.add_class::<TerminalReference>()?;
    m.add_class::<Connection>()?;
    m.add_class::<Bus>()?;
    m.add_class::<ExternalModule>()?;
    m.add_class::<ProtoModuleReference>()?;
    m.add_class::<ProtoModule>()?;
    m.add_class::<ProtoCircuit>()?;
    m.add_class::<ArraySpec>()?;
    m.add_class::<Instance>()?;
    m.add_class::<Module>()?;
    m.add_class::<TopLevelModule>()?;
    m.add_class::<TerminalDirection>()?;
    m.add_class::<SignalDomain>()?;
    m.add_class::<SIPrefix>()?;
    m.add_function(wrap_pyfunction!(load_pic_yaml, m)?)?;
    Ok(())
}
