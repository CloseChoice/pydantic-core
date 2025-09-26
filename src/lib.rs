#![cfg_attr(has_coverage_attribute, feature(coverage_attribute))]

extern crate core;

use std::sync::OnceLock;

use jiter::{map_json_error, FloatMode, PartialMode, PythonParse, StringCacheMode};
use pyo3::exceptions::PyTypeError;
use pyo3::{prelude::*, sync::PyOnceLock};
use serializers::BytesMode;
use validators::ValBytesMode;

// parse this first to get access to the contained macro
#[macro_use]
mod py_gc;

mod argument_markers;
mod build_tools;
mod common;
mod definitions;
mod errors;
mod input;
mod lookup_key;
mod recursion_guard;
mod serializers;
mod tools;
mod url;
mod validators;

// required for benchmarks
pub use self::input::TzInfo;
pub use self::url::{PyMultiHostUrl, PyUrl};
pub use argument_markers::{ArgsKwargs, PydanticUndefinedType};
pub use build_tools::SchemaError;
pub use errors::{
    list_all_errors, PydanticCustomError, PydanticKnownError, PydanticOmit, PydanticUseDefault, ValidationError,
};
pub use serializers::{
    to_json, to_jsonable_python, PydanticSerializationError, PydanticSerializationUnexpectedValue, SchemaSerializer,
    WarningsArg,
};
pub use validators::{PySome, SchemaValidator};

use crate::input::Input;

// TODO: REMOVE THIS TEST FUNCTION BEFORE PRODUCTION
/// Test function to examine how Rust iterates over Python OrderedDict
/// and create new dictionaries with different approaches
#[pyfunction]
pub fn test_ordered_dict_iteration(py: Python, obj: &Bound<PyAny>) -> PyResult<Py<PyAny>> {
    use pyo3::types::{PyDict, PyMapping, PyTuple};

    let result_dict = PyDict::new(py);

    // Add type information
    result_dict.set_item("input_type", obj.get_type().name()?)?;

    // Test 1: Create PyDict from PyDict iteration (loses order for OrderedDict)
    if let Ok(dict) = obj.downcast::<PyDict>() {
        let pydict_from_pydict = PyDict::new(py);
        let mut pydict_keys = Vec::new();

        for (i, (key, value)) in dict.iter().enumerate() {
            pydict_from_pydict.set_item(&key, &value)?;
            pydict_keys.push(format!("{}:{}", i, key.repr()?));
        }

        result_dict.set_item("pydict_from_pydict_iteration", pydict_from_pydict)?;
        result_dict.set_item("pydict_iteration_order", pydict_keys)?;
    }

    // Test 2: Create PyDict from PyMapping iteration (preserves order)
    if let Ok(mapping) = obj.downcast::<PyMapping>() {
        let pydict_from_mapping = PyDict::new(py);
        let mut mapping_keys = Vec::new();

        let items = mapping.items()?;
        for (i, item) in items.iter().enumerate() {
            let tuple = item.downcast::<PyTuple>()?;
            let key = tuple.get_item(0)?;
            let value = tuple.get_item(1)?;
            pydict_from_mapping.set_item(&key, &value)?;
            mapping_keys.push(format!("{}:{}", i, key.repr()?));
        }

        result_dict.set_item("pydict_from_mapping_iteration", pydict_from_mapping)?;
        result_dict.set_item("mapping_iteration_order", mapping_keys)?;
    }

    // Test 3: Create OrderedDict from PyMapping iteration
    if let Ok(mapping) = obj.downcast::<PyMapping>() {
        let collections = py.import("collections")?;
        let ordered_dict_type = collections.getattr("OrderedDict")?;
        let ordered_dict_from_mapping = ordered_dict_type.call0()?;
        let mut ordered_keys = Vec::new();

        let items = mapping.items()?;
        for (i, item) in items.iter().enumerate() {
            let tuple = item.downcast::<PyTuple>()?;
            let key = tuple.get_item(0)?;
            let value = tuple.get_item(1)?;
            ordered_dict_from_mapping.call_method1("__setitem__", (&key, &value))?;
            ordered_keys.push(format!("{}:{}", i, key.repr()?));
        }

        result_dict.set_item("ordereddict_from_mapping_iteration", ordered_dict_from_mapping)?;
        result_dict.set_item("ordereddict_iteration_order", ordered_keys)?;
    }

    // Test 4: Check if it's OrderedDict specifically
    let collections = py.import("collections")?;
    let ordered_dict_type = collections.getattr("OrderedDict")?;
    result_dict.set_item("is_ordered_dict", obj.is_instance(&ordered_dict_type)?)?;

    // Test 5: Original keys order
    if let Ok(keys) = obj.call_method0("keys") {
        let mut original_keys = Vec::new();
        for (i, key) in keys.try_iter()?.enumerate() {
            let key = key?;
            original_keys.push(format!("{}:{}", i, key.repr()?));
        }
        result_dict.set_item("original_keys_order", original_keys)?;
    }

    Ok(result_dict.into())
}

#[pyfunction(signature = (data, *, allow_inf_nan=true, cache_strings=StringCacheMode::All, allow_partial=PartialMode::Off))]
pub fn from_json<'py>(
    py: Python<'py>,
    data: &Bound<'_, PyAny>,
    allow_inf_nan: bool,
    cache_strings: StringCacheMode,
    allow_partial: PartialMode,
) -> PyResult<Bound<'py, PyAny>> {
    let v_match = data
        .validate_bytes(false, ValBytesMode { ser: BytesMode::Utf8 })
        .map_err(|_| PyTypeError::new_err("Expected bytes, bytearray or str"))?;
    let json_either_bytes = v_match.into_inner();
    let json_bytes = json_either_bytes.as_slice();
    let parse_builder = PythonParse {
        allow_inf_nan,
        cache_mode: cache_strings,
        partial_mode: allow_partial,
        catch_duplicate_keys: false,
        float_mode: FloatMode::Float,
    };
    parse_builder
        .python_parse(py, json_bytes)
        .map_err(|e| map_json_error(json_bytes, &e))
}

pub fn get_pydantic_core_version() -> &'static str {
    static PYDANTIC_CORE_VERSION: OnceLock<String> = OnceLock::new();

    PYDANTIC_CORE_VERSION.get_or_init(|| {
        let version = env!("CARGO_PKG_VERSION");
        // cargo uses "1.0-alpha1" etc. while python uses "1.0.0a1", this is not full compatibility,
        // but it's good enough for now
        // see https://docs.rs/semver/1.0.9/semver/struct.Version.html#method.parse for rust spec
        // see https://peps.python.org/pep-0440/ for python spec
        // it seems the dot after "alpha/beta" e.g. "-alpha.1" is not necessary, hence why this works
        version.replace("-alpha", "a").replace("-beta", "b")
    })
}

/// Returns the installed version of pydantic.
fn get_pydantic_version(py: Python<'_>) -> Option<&'static str> {
    static PYDANTIC_VERSION: PyOnceLock<Option<String>> = PyOnceLock::new();

    PYDANTIC_VERSION
        .get_or_init(py, || {
            py.import("pydantic")
                .and_then(|pydantic| pydantic.getattr("__version__")?.extract())
                .ok()
        })
        .as_deref()
}

pub fn build_info() -> String {
    format!(
        "profile={} pgo={}",
        env!("PROFILE"),
        // We use a `cfg!` here not `env!`/`option_env!` as those would
        // embed `RUSTFLAGS` into the generated binary which causes problems
        // with reproducable builds.
        cfg!(specified_profile_use),
    )
}

#[pymodule(gil_used = false)]
mod _pydantic_core {
    #[allow(clippy::wildcard_imports)]
    use super::*;

    #[pymodule_export]
    use crate::{
        from_json, list_all_errors, test_ordered_dict_iteration, to_json, to_jsonable_python, ArgsKwargs, PyMultiHostUrl, PySome, PyUrl,
        PydanticCustomError, PydanticKnownError, PydanticOmit, PydanticSerializationError,
        PydanticSerializationUnexpectedValue, PydanticUndefinedType, PydanticUseDefault, SchemaError, SchemaSerializer,
        SchemaValidator, TzInfo, ValidationError,
    };

    #[pymodule_init]
    fn module_init(m: &Bound<'_, PyModule>) -> PyResult<()> {
        m.add("__version__", get_pydantic_core_version())?;
        m.add("build_profile", env!("PROFILE"))?;
        m.add("build_info", build_info())?;
        m.add("_recursion_limit", recursion_guard::RECURSION_GUARD_LIMIT)?;
        m.add("PydanticUndefined", PydanticUndefinedType::new(m.py()))?;
        Ok(())
    }
}
