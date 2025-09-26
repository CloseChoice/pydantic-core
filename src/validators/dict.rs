use pyo3::intern;
use pyo3::prelude::*;
use pyo3::sync::PyOnceLock;
use pyo3::types::{PyDict, PyType};

use crate::build_tools::is_strict;
use crate::errors::{ErrorType, LocItem, ValError, ValLineError, ValResult};
use crate::input::BorrowInput;
use crate::input::ConsumeIterator;
use crate::input::{Input, ValidatedDict};

use crate::tools::SchemaDict;

use super::any::AnyValidator;
use super::{build_validator, BuildValidator, CombinedValidator, DefinitionsBuilder, ValidationState, Validator};

static ORDERED_DICT_TYPE: PyOnceLock<Py<PyType>> = PyOnceLock::new();

pub fn get_ordered_dict_type(py: Python<'_>) -> &Bound<'_, PyType> {
    ORDERED_DICT_TYPE
        .get_or_init(py, || {
            py.import("collections")
                .and_then(|collections_module| collections_module.getattr("OrderedDict"))
                .unwrap()
                .extract()
                .unwrap()
        })
        .bind(py)
}

#[derive(Debug)]
pub struct DictValidator {
    strict: bool,
    key_validator: Box<CombinedValidator>,
    value_validator: Box<CombinedValidator>,
    min_length: Option<usize>,
    max_length: Option<usize>,
    name: String,
}

impl BuildValidator for DictValidator {
    const EXPECTED_TYPE: &'static str = "dict";

    fn build(
        schema: &Bound<'_, PyDict>,
        config: Option<&Bound<'_, PyDict>>,
        definitions: &mut DefinitionsBuilder<CombinedValidator>,
    ) -> PyResult<CombinedValidator> {
        let py = schema.py();
        let key_validator = match schema.get_item(intern!(py, "keys_schema"))? {
            Some(schema) => Box::new(build_validator(&schema, config, definitions)?),
            None => Box::new(AnyValidator::build(schema, config, definitions)?),
        };
        let value_validator = match schema.get_item(intern!(py, "values_schema"))? {
            Some(d) => Box::new(build_validator(&d, config, definitions)?),
            None => Box::new(AnyValidator::build(schema, config, definitions)?),
        };
        let name = format!(
            "{}[{},{}]",
            Self::EXPECTED_TYPE,
            key_validator.get_name(),
            value_validator.get_name()
        );
        Ok(Self {
            strict: is_strict(schema, config)?,
            key_validator,
            value_validator,
            min_length: schema.get_as(intern!(py, "min_length"))?,
            max_length: schema.get_as(intern!(py, "max_length"))?,
            name,
        }
        .into())
    }
}

impl_py_gc_traverse!(DictValidator {
    key_validator,
    value_validator
});

impl Validator for DictValidator {
    fn validate<'py>(
        &self,
        py: Python<'py>,
        input: &(impl Input<'py> + ?Sized),
        state: &mut ValidationState<'_, 'py>,
    ) -> ValResult<Py<PyAny>> {
        let strict = state.strict_or(self.strict);
        let dict = input.validate_dict(strict)?;
        dict.iterate(ValidateToDict {
            py,
            input,
            min_length: self.min_length,
            max_length: self.max_length,
            key_validator: &self.key_validator,
            value_validator: &self.value_validator,
            state,
        })?
    }

    fn get_name(&self) -> &str {
        &self.name
    }
}

struct ValidateToDict<'a, 's, 'py, I: Input<'py> + ?Sized> {
    py: Python<'py>,
    input: &'a I,
    min_length: Option<usize>,
    max_length: Option<usize>,
    key_validator: &'a CombinedValidator,
    value_validator: &'a CombinedValidator,
    state: &'a mut ValidationState<'s, 'py>,
}

impl<'py, Key, Value, I: Input<'py> + ?Sized> ConsumeIterator<ValResult<(Key, Value)>>
    for ValidateToDict<'_, '_, 'py, I>
where
    Key: BorrowInput<'py> + Clone + Into<LocItem>,
    Value: BorrowInput<'py>,
{
    type Output = ValResult<Py<PyAny>>;
    fn consume_iterator(self, iterator: impl Iterator<Item = ValResult<(Key, Value)>>) -> ValResult<Py<PyAny>> {
        // Check if input was an OrderedDict and preserve the type
        let is_ordered_dict = if let Some(py_input) = self.input.as_python() {
            let ordered_dict_type = get_ordered_dict_type(self.py);
            py_input.is_instance(ordered_dict_type).unwrap_or(false)
        } else {
            false
        };

        let output: Bound<PyAny> = if is_ordered_dict {
            // Create OrderedDict() - call the constructor with empty args
            let ordered_dict_type = get_ordered_dict_type(self.py);
            ordered_dict_type.call0()?.into_any()
        } else {
            PyDict::new(self.py).into_any()
        };

        let mut errors: Vec<ValLineError> = Vec::new();
        let allow_partial = self.state.allow_partial;

        for (_, is_last_partial, item_result) in self.state.enumerate_last_partial(iterator) {
            self.state.allow_partial = false.into();
            let (key, value) = item_result?;
            let output_key = match self.key_validator.validate(self.py, key.borrow_input(), self.state) {
                Ok(value) => Some(value),
                Err(ValError::LineErrors(line_errors)) => {
                    for err in line_errors {
                        // these are added in reverse order so [key] is shunted along by the second call
                        errors.push(err.with_outer_location("[key]").with_outer_location(key.clone()));
                    }
                    None
                }
                Err(ValError::Omit) => continue,
                Err(err) => return Err(err),
            };
            self.state.allow_partial = match is_last_partial {
                true => allow_partial,
                false => false.into(),
            };
            let output_value = match self.value_validator.validate(self.py, value.borrow_input(), self.state) {
                Ok(value) => value,
                Err(ValError::LineErrors(line_errors)) => {
                    if !is_last_partial {
                        errors.extend(line_errors.into_iter().map(|err| err.with_outer_location(key.clone())));
                    }
                    continue;
                }
                Err(ValError::Omit) => continue,
                Err(err) => return Err(err),
            };
            if let Some(key) = output_key {
                output.set_item(key, output_value)?;
            }
        }

        if errors.is_empty() {
            let input = self.input;
            // Manual length check since we have Bound<PyAny> instead of PyDict
            let mut op_actual_length: Option<usize> = None;
            if let Some(min_length) = self.min_length {
                let actual_length = output.len()?;
                if actual_length < min_length {
                    return Err(ValError::new(
                        ErrorType::TooShort {
                            field_type: "Dictionary".to_string(),
                            min_length,
                            actual_length,
                            context: None,
                        },
                        input,
                    ));
                }
                op_actual_length = Some(actual_length);
            }
            if let Some(max_length) = self.max_length {
                let actual_length = op_actual_length.unwrap_or_else(|| output.len().unwrap_or(0));
                if actual_length > max_length {
                    return Err(ValError::new(
                        ErrorType::TooLong {
                            field_type: "Dictionary".to_string(),
                            max_length,
                            actual_length: Some(actual_length),
                            context: None,
                        },
                        input,
                    ));
                }
            }
            Ok(output.into())
        } else {
            Err(ValError::LineErrors(errors))
        }
    }
}
