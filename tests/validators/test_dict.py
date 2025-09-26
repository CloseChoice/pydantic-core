import re
from collections import OrderedDict
from collections.abc import Mapping
from typing import Any

import pytest
from dirty_equals import HasRepr, IsStr

from pydantic_core import SchemaValidator, ValidationError
from pydantic_core import core_schema as cs

from ..conftest import Err, PyAndJson


def test_dict(py_and_json: PyAndJson):
    v = py_and_json({'type': 'dict', 'keys_schema': {'type': 'int'}, 'values_schema': {'type': 'int'}})
    assert v.validate_test({'1': 2, '3': 4}) == {1: 2, 3: 4}
    v = py_and_json({'type': 'dict', 'strict': True, 'keys_schema': {'type': 'int'}, 'values_schema': {'type': 'int'}})
    assert v.validate_test({'1': 2, '3': 4}) == {1: 2, 3: 4}
    assert v.validate_test({}) == {}
    with pytest.raises(ValidationError, match=re.escape('[type=dict_type, input_value=[], input_type=list]')):
        v.validate_test([])


@pytest.mark.parametrize(
    'input_value,expected',
    [
        ({'1': b'1', '2': b'2'}, {'1': '1', '2': '2'}),
        (OrderedDict(a=b'1', b='2'), {'a': '1', 'b': '2'}),
        ({}, {}),
        ('foobar', Err("Input should be a valid dictionary [type=dict_type, input_value='foobar', input_type=str]")),
        ([], Err('Input should be a valid dictionary [type=dict_type,')),
        ([('x', 'y')], Err('Input should be a valid dictionary [type=dict_type,')),
        ([('x', 'y'), ('z', 'z')], Err('Input should be a valid dictionary [type=dict_type,')),
        ((), Err('Input should be a valid dictionary [type=dict_type,')),
        ((('x', 'y'),), Err('Input should be a valid dictionary [type=dict_type,')),
        ((type('Foobar', (), {'x': 1})()), Err('Input should be a valid dictionary [type=dict_type,')),
    ],
    ids=repr,
)
def test_dict_cases(input_value, expected):
    v = SchemaValidator(cs.dict_schema(keys_schema=cs.str_schema(), values_schema=cs.str_schema()))
    if isinstance(expected, Err):
        with pytest.raises(ValidationError, match=re.escape(expected.message)):
            v.validate_python(input_value)
    else:
        assert v.validate_python(input_value) == expected


def test_dict_value_error(py_and_json: PyAndJson):
    v = py_and_json({'type': 'dict', 'values_schema': {'type': 'int'}})
    assert v.validate_test({'a': 2, 'b': '4'}) == {'a': 2, 'b': 4}
    with pytest.raises(ValidationError, match='Input should be a valid integer') as exc_info:
        v.validate_test({'a': 2, 'b': 'wrong'})
    assert exc_info.value.errors(include_url=False) == [
        {
            'type': 'int_parsing',
            'loc': ('b',),
            'msg': 'Input should be a valid integer, unable to parse string as an integer',
            'input': 'wrong',
        }
    ]


def test_dict_error_key_int():
    v = SchemaValidator(cs.dict_schema(values_schema=cs.int_schema()))
    with pytest.raises(ValidationError, match='Input should be a valid integer') as exc_info:
        v.validate_python({1: 2, 3: 'wrong', -4: 'wrong2'})
    # insert_assert(exc_info.value.errors(include_url=False))
    assert exc_info.value.errors(include_url=False) == [
        {
            'type': 'int_parsing',
            'loc': (3,),
            'msg': 'Input should be a valid integer, unable to parse string as an integer',
            'input': 'wrong',
        },
        {
            'type': 'int_parsing',
            'loc': (-4,),
            'msg': 'Input should be a valid integer, unable to parse string as an integer',
            'input': 'wrong2',
        },
    ]


def test_dict_error_key_other():
    v = SchemaValidator(cs.dict_schema(values_schema=cs.int_schema()))
    with pytest.raises(ValidationError, match='Input should be a valid integer') as exc_info:
        v.validate_python({1: 2, (1, 2): 'wrong'})
    assert exc_info.value.errors(include_url=False) == [
        {
            'type': 'int_parsing',
            'loc': ('(1, 2)',),
            'msg': 'Input should be a valid integer, unable to parse string as an integer',
            'input': 'wrong',
        }
    ]


def test_dict_any_value():
    v = SchemaValidator(cs.dict_schema(keys_schema=cs.str_schema()))
    v = SchemaValidator(cs.dict_schema(keys_schema=cs.str_schema()))
    assert v.validate_python({'1': 1, '2': 'a', '3': None}) == {'1': 1, '2': 'a', '3': None}


def test_mapping():
    class MyMapping(Mapping):
        def __init__(self, d):
            self._d = d

        def __getitem__(self, key):
            return self._d[key]

        def __iter__(self):
            return iter(self._d)

        def __len__(self):
            return len(self._d)

    v = SchemaValidator(cs.dict_schema(keys_schema=cs.int_schema(), values_schema=cs.int_schema()))
    assert v.validate_python(MyMapping({'1': 2, 3: '4'})) == {1: 2, 3: 4}
    v = SchemaValidator(cs.dict_schema(strict=True, keys_schema=cs.int_schema(), values_schema=cs.int_schema()))
    with pytest.raises(ValidationError, match='Input should be a valid dictionary'):
        v.validate_python(MyMapping({'1': 2, 3: '4'}))


def test_key_error():
    v = SchemaValidator(cs.dict_schema(keys_schema=cs.int_schema(), values_schema=cs.int_schema()))
    assert v.validate_python({'1': True}) == {1: 1}
    with pytest.raises(ValidationError, match=re.escape('x.[key]\n  Input should be a valid integer')) as exc_info:
        v.validate_python({'x': 1})
    assert exc_info.value.errors(include_url=False) == [
        {
            'type': 'int_parsing',
            'loc': ('x', '[key]'),
            'msg': 'Input should be a valid integer, unable to parse string as an integer',
            'input': 'x',
        }
    ]


def test_mapping_error():
    class BadMapping(Mapping):
        def __getitem__(self, key):
            raise None

        def __iter__(self):
            raise RuntimeError('intentional error')

        def __len__(self):
            return 1

    v = SchemaValidator(cs.dict_schema(keys_schema=cs.int_schema(), values_schema=cs.int_schema()))
    with pytest.raises(ValidationError) as exc_info:
        v.validate_python(BadMapping())

    assert exc_info.value.errors(include_url=False) == [
        {
            'type': 'mapping_type',
            'loc': (),
            'msg': 'Input should be a valid mapping, error: RuntimeError: intentional error',
            'input': HasRepr(IsStr(regex='.+BadMapping object at.+')),
            'ctx': {'error': 'RuntimeError: intentional error'},
        }
    ]


@pytest.mark.parametrize('mapping_items', [[(1,)], ['foobar'], [(1, 2, 3)], 'not list'])
def test_mapping_error_yield_1(mapping_items):
    class BadMapping(Mapping):
        def items(self):
            return mapping_items

        def __iter__(self):
            pytest.fail('unexpected call to __iter__')

        def __getitem__(self, key):
            pytest.fail('unexpected call to __getitem__')

        def __len__(self):
            return 1

    v = SchemaValidator(cs.dict_schema(keys_schema=cs.int_schema(), values_schema=cs.int_schema()))
    with pytest.raises(ValidationError) as exc_info:
        v.validate_python(BadMapping())

    assert exc_info.value.errors(include_url=False) == [
        {
            'type': 'mapping_type',
            'loc': (),
            'msg': 'Input should be a valid mapping, error: Mapping items must be tuples of (key, value) pairs',
            'input': HasRepr(IsStr(regex='.+BadMapping object at.+')),
            'ctx': {'error': 'Mapping items must be tuples of (key, value) pairs'},
        }
    ]


@pytest.mark.parametrize(
    'kwargs,input_value,expected',
    [
        ({}, {'1': 1, '2': 2}, {'1': 1, '2': 2}),
        (
            {'min_length': 3},
            {'1': 1, '2': 2, '3': 3.0, '4': [1, 2, 3, 4]},
            {'1': 1, '2': 2, '3': 3.0, '4': [1, 2, 3, 4]},
        ),
        (
            {'min_length': 3},
            {1: '2', 3: '4'},
            Err('Dictionary should have at least 3 items after validation, not 2 [type=too_short,'),
        ),
        ({'max_length': 4}, {'1': 1, '2': 2, '3': 3.0}, {'1': 1, '2': 2, '3': 3.0}),
        (
            {'max_length': 3},
            {'1': 1, '2': 2, '3': 3.0, '4': [1, 2, 3, 4]},
            Err('Dictionary should have at most 3 items after validation, not 4 [type=too_long,'),
        ),
    ],
)
def test_dict_length_constraints(kwargs: dict[str, Any], input_value, expected):
    v = SchemaValidator(cs.dict_schema(**kwargs))
    if isinstance(expected, Err):
        with pytest.raises(ValidationError, match=re.escape(expected.message)):
            v.validate_python(input_value)
    else:
        assert v.validate_python(input_value) == expected


def test_json_dict():
    v = SchemaValidator(cs.dict_schema(keys_schema=cs.int_schema(), values_schema=cs.int_schema()))
    assert v.validate_json('{"1": 2, "3": 4}') == {1: 2, 3: 4}
    with pytest.raises(ValidationError) as exc_info:
        v.validate_json('1')
    assert exc_info.value.errors(include_url=False) == [
        {'type': 'dict_type', 'loc': (), 'msg': 'Input should be an object', 'input': 1}
    ]


def test_dict_complex_key():
    v = SchemaValidator(cs.dict_schema(keys_schema=cs.complex_schema(strict=True), values_schema=cs.str_schema()))
    assert v.validate_python({complex(1, 2): '1'}) == {complex(1, 2): '1'}
    with pytest.raises(ValidationError, match='Input should be an instance of complex'):
        assert v.validate_python({'1+2j': b'1'}) == {complex(1, 2): '1'}

    v = SchemaValidator(cs.dict_schema(keys_schema=cs.complex_schema(), values_schema=cs.str_schema()))
    with pytest.raises(
        ValidationError, match='Input should be a valid python complex object, a number, or a valid complex string'
    ):
        v.validate_python({'1+2ja': b'1'})


def test_json_dict_complex_key():
    v = SchemaValidator(cs.dict_schema(keys_schema=cs.complex_schema(), values_schema=cs.int_schema()))
    assert v.validate_json('{"1+2j": 2, "-3": 4}') == {complex(1, 2): 2, complex(-3, 0): 4}
    assert v.validate_json('{"1+2j": 2, "infj": 4}') == {complex(1, 2): 2, complex(0, float('inf')): 4}
    with pytest.raises(ValidationError, match='Input should be a valid complex string'):
        v.validate_json('{"1+2j": 2, "": 4}') == {complex(1, 2): 2, complex(0, float('inf')): 4}


def test_ordered_dict_key_order_preservation():
    """Test that OrderedDict key ordering is preserved during validation.

    This test addresses GitHub issue #12273 where OrderedDict key ordering
    was not preserved when validating with Pydantic V2.
    """
    # Test with simple string keys and int values
    v = SchemaValidator(cs.dict_schema(keys_schema=cs.str_schema(), values_schema=cs.int_schema()))

    # Test case from original issue
    foo = OrderedDict({"a": 1, "b": 2})
    foo.move_to_end("a")  # Order should now be: ['b', 'a']

    result = v.validate_python(foo)
    assert list(result.keys()) == list(foo.keys()) == ['b', 'a']
    assert result == {'b': 2, 'a': 1}

    # Test with more complex reordering
    foo2 = OrderedDict({"x": 1, "y": 2, "z": 3})
    foo2.move_to_end("x")  # Order should be: ['y', 'z', 'x']

    result2 = v.validate_python(foo2)
    assert list(result2.keys()) == list(foo2.keys()) == ['y', 'z', 'x']
    assert result2 == {'y': 2, 'z': 3, 'x': 1}

    # Test popitem and re-insertion
    foo3 = OrderedDict({"p": 1, "q": 2})
    item = foo3.popitem(last=False)  # Remove first item ('p', 1)
    foo3[item[0]] = item[1]  # Re-insert at end: ['q', 'p']

    result3 = v.validate_python(foo3)
    assert list(result3.keys()) == list(foo3.keys()) == ['q', 'p']
    assert result3 == {'q': 2, 'p': 1}


def test_ordered_dict_with_different_value_types():
    """Test OrderedDict ordering with different value types."""
    v = SchemaValidator(cs.dict_schema(keys_schema=cs.str_schema(), values_schema=cs.any_schema()))

    foo = OrderedDict([("first", "string"), ("second", 42), ("third", [1, 2, 3])])
    foo.move_to_end("first")  # Order: ['second', 'third', 'first']

    result = v.validate_python(foo)
    assert list(result.keys()) == list(foo.keys()) == ['second', 'third', 'first']
    assert result == {'second': 42, 'third': [1, 2, 3], 'first': 'string'}


def test_ordered_dict_vs_regular_dict():
    """Test that regular dict behavior is unchanged."""
    v = SchemaValidator(cs.dict_schema(keys_schema=cs.str_schema(), values_schema=cs.int_schema()))

    # Regular dict - order may or may not be preserved (implementation dependent for older Python)
    regular_dict = {"a": 1, "b": 2}
    result_regular = v.validate_python(regular_dict)
    assert result_regular == regular_dict

    # OrderedDict - order must be preserved
    ordered_dict = OrderedDict({"a": 1, "b": 2})
    ordered_dict.move_to_end("a")
    result_ordered = v.validate_python(ordered_dict)
    assert list(result_ordered.keys()) == list(ordered_dict.keys()) == ['b', 'a']


def test_ordered_dict_original_issue_case():
    """Test exact case from GitHub issue #12273.

    This reproduces the exact code that was failing in the original issue report.
    """
    # Create validator that accepts OrderedDict[str, int]
    v = SchemaValidator(cs.dict_schema(keys_schema=cs.str_schema(), values_schema=cs.int_schema()))

    # Reproduce the exact issue scenario
    foo = OrderedDict({"a": 1, "b": 2})
    foo.move_to_end("a")

    # This should work now (was failing before the fix)
    model1_result = v.validate_python(foo)

    # This always worked (using dict() constructor preserves order)
    model2_result = v.validate_python(dict(foo))

    # Both should preserve the same order and have the same content
    expected_keys = ['b', 'a']
    expected_dict = {'b': 2, 'a': 1}

    assert list(foo.keys()) == expected_keys
    assert list(model1_result.keys()) == expected_keys  # This was failing before
    assert list(model2_result.keys()) == expected_keys

    assert model1_result == expected_dict
    assert model2_result == expected_dict

    # All three should have the same key order
    assert list(foo.keys()) == list(model1_result.keys()) == list(model2_result.keys())
