use super::{bson_to_json, json_to_bson};
use super::{build_filter, escape_regex, parse_regex, parse_value, Filter};

fn filter(field: &str, operator: &str, value: Option<&str>) -> Filter {
    Filter {
        field: field.to_string(),
        operator: operator.to_string(),
        value: value.map(str::to_string),
    }
}

/// What the text in a filter box is read as. Mongo matches by exact BSON type, so this is the
/// whole difference between finding a document and finding nothing: `{_id: "5"}` matches
/// nothing in a collection keyed by the number 5.
#[test]
fn a_filter_value_is_read_as_the_bson_type_it_looks_like() {
    assert_eq!(parse_value("null"), Bson::Null);
    assert_eq!(parse_value("true"), Bson::Boolean(true));
    assert_eq!(parse_value("false"), Bson::Boolean(false));

    // Whole numbers stay small where they fit, because that is the type they were stored as.
    assert_eq!(parse_value("5"), Bson::Int32(5));
    assert_eq!(parse_value("-5"), Bson::Int32(-5));
    assert_eq!(parse_value("2147483648"), Bson::Int64(2_147_483_648));
    assert_eq!(parse_value("1.5"), Bson::Double(1.5));

    // A number needs a digit: `inf` and `NaN` parse as f64, and nobody typing either into a
    // filter box means the floating-point value.
    assert_eq!(parse_value("inf"), Bson::String("inf".to_string()));
    assert_eq!(parse_value("NaN"), Bson::String("NaN".to_string()));

    // 24 hex characters is what an ObjectId looks like, which is what `_id` usually holds.
    assert_eq!(
        parse_value("507f1f77bcf86cd799439011"),
        Bson::ObjectId(ObjectId::from_str("507f1f77bcf86cd799439011").unwrap()),
    );
    // 23 of them is a string, not a malformed id.
    assert_eq!(
        parse_value("507f1f77bcf86cd79943901"),
        Bson::String("507f1f77bcf86cd79943901".to_string()),
    );

    assert!(matches!(
        parse_value("2024-01-02T03:04:05Z"),
        Bson::DateTime(_)
    ));

    // Quoting turns all of it off, which is how a field that really does hold "5" is reached.
    assert_eq!(parse_value("'5'"), Bson::String("5".to_string()));
    assert_eq!(parse_value("'null'"), Bson::String("null".to_string()));

    // Text keeps the spaces around it — they are part of it, and the quoted form is there for
    // when they are not meant.
    assert_eq!(parse_value("  hi  "), Bson::String("  hi  ".to_string()));
}

/// `/pattern/flags` is how a regex is written everywhere else, and the only way to ask for one
/// of Mongo's flags from a single text box.
#[test]
fn a_regex_is_read_with_its_flags_and_without_the_ones_mongo_has_no_use_for() {
    let plain = parse_regex("^a.*b$");
    assert_eq!(plain.pattern, "^a.*b$");
    assert_eq!(plain.options, "");

    let delimited = parse_regex("/^ab$/i");
    assert_eq!(delimited.pattern, "^ab$");
    assert_eq!(delimited.options, "i");

    // A `g` carried over from JavaScript habit is dropped rather than passed on, which would
    // have the server reject the whole query.
    assert_eq!(parse_regex("/ab/gimu").options, "imu");

    // The *last* slash closes it, so a pattern holding one still parses.
    let with_slash = parse_regex("/a/b/i");
    assert_eq!(with_slash.pattern, "a/b");
    assert_eq!(with_slash.options, "i");

    // An opening slash with no closing one is not a delimited regex; it is a pattern that
    // happens to start with a slash.
    assert_eq!(parse_regex("/ab").pattern, "/ab");
}

/// A value going into a built pattern is matched as itself, not as a pattern of its own.
#[test]
fn a_value_put_into_a_regex_is_escaped_first() {
    assert_eq!(escape_regex("a.b"), "a\\.b");
    assert_eq!(escape_regex("1+1"), "1\\+1");
    assert_eq!(escape_regex(".*"), "\\.\\*");
    assert_eq!(escape_regex("plain"), "plain");
}

/// The document a row turns into, and the rows that turn into nothing.
#[test]
fn a_filter_row_becomes_the_clause_it_means() {
    // Nothing to say yet: the bar opens with an empty row, which must not become `{_id: ""}`.
    assert_eq!(build_filter(&[]).unwrap(), doc! {});
    assert_eq!(
        build_filter(&[filter("  ", "eq", Some("x"))]).unwrap(),
        doc! {}
    );

    assert_eq!(
        build_filter(&[filter("age", "gte", Some("18"))]).unwrap(),
        doc! { "$and": [ { "age": { "$gte": 18i32 } } ] },
    );

    // Gathered under `$and` so that two conditions on one field both survive — an object
    // holds one entry per key, and `{age: .., age: ..}` would silently be one of them.
    let both = build_filter(&[
        filter("age", "gte", Some("18")),
        filter("age", "lte", Some("30")),
    ])
    .unwrap();
    assert_eq!(
        both,
        doc! { "$and": [ { "age": { "$gte": 18i32 } }, { "age": { "$lte": 30i32 } } ] },
    );

    // `startsWith` builds its own pattern, and escapes the value going into it.
    assert_eq!(
        build_filter(&[filter("name", "startsWith", Some("a.b"))]).unwrap(),
        doc! { "$and": [ { "name": Bson::RegularExpression(Regex {
            pattern: "^a\\.b".to_string(),
            options: "i".to_string(),
        }) } ] },
    );

    // `isNull` is not `{field: null}`, which would also match documents missing the field —
    // the opposite of what a schemaless collection needs to be able to ask.
    assert_eq!(
        build_filter(&[filter("nickname", "isNull", None)]).unwrap(),
        doc! { "$and": [ { "nickname": { "$type": "null" } } ] },
    );

    // A list of one bound is not a range, and an empty list is not a set.
    assert_eq!(
        build_filter(&[filter("age", "between", Some("18"))]).unwrap(),
        doc! {}
    );
    assert_eq!(
        build_filter(&[filter("age", "in", Some("  "))]).unwrap(),
        doc! {}
    );
}

/// What a filter refuses. A leading `$` would make the field name read as a query operator,
/// which is not a document this bar has any way to mean.
#[test]
fn a_filter_refuses_an_operator_field_and_an_unknown_operator() {
    assert!(build_filter(&[filter("$where", "eq", Some("1"))]).is_err());
    assert!(build_filter(&[filter("age", "approximately", Some("18"))]).is_err());
}

use mongodb::bson::{
    doc, oid::ObjectId, spec::BinarySubtype, Binary, Bson, DateTime as BsonDateTime, Decimal128,
    Regex, Timestamp,
};
use serde_json::json;
use std::str::FromStr;

#[track_caller]
fn round_trips(value: Bson) {
    let back = json_to_bson(&bson_to_json(&value)).unwrap();
    assert_eq!(back, value);
}

#[test]
fn the_types_json_spells_the_same_way_survive_the_round_trip() {
    round_trips(Bson::String("text".into()));
    round_trips(Bson::Boolean(true));
    round_trips(Bson::Null);
    round_trips(Bson::Array(vec![Bson::Int32(1), Bson::String("a".into())]));
    round_trips(Bson::Document(doc! { "a": 1_i32, "b": { "c": true } }));
}

/// The numbers are the reason this converter exists at all: Int32, Int64 and Double are one
/// and the same thing in JSON, and a document read as one and written back as another is a
/// document whose schema has silently changed.
#[test]
fn the_three_number_types_stay_apart() {
    round_trips(Bson::Int32(7));
    round_trips(Bson::Int64(i64::MAX));
    round_trips(Bson::Double(1.5));
    round_trips(Bson::Decimal128(Decimal128::from_str("1.250").unwrap()));

    // An Int64 too large for a double is carried as text, so it comes back exact rather than
    // rounded to the nearest representable value.
    let big = Bson::Int64(9_007_199_254_740_993);
    assert_eq!(json_to_bson(&bson_to_json(&big)).unwrap(), big);
}

#[test]
fn the_types_json_has_no_spelling_for_survive_too() {
    round_trips(Bson::ObjectId(
        ObjectId::parse_str("65a1b2c3d4e5f60718293a4b").unwrap(),
    ));
    round_trips(Bson::DateTime(BsonDateTime::from_millis(1_700_000_000_123)));
    round_trips(Bson::Timestamp(Timestamp {
        time: 42,
        increment: 7,
    }));
    round_trips(Bson::Binary(Binary {
        subtype: BinarySubtype::Generic,
        bytes: vec![0, 1, 2, 255],
    }));
    round_trips(Bson::RegularExpression(Regex {
        pattern: "^a.*z$".into(),
        options: "i".into(),
    }));
    round_trips(Bson::JavaScriptCode("return 1".into()));
    round_trips(Bson::Symbol("sym".into()));
    round_trips(Bson::Undefined);
    round_trips(Bson::MinKey);
    round_trips(Bson::MaxKey);
}

/// A date is written with three fractional digits whatever the instant, so an untouched date
/// reads back as the same string it was shown as rather than looking edited.
#[test]
fn a_whole_second_date_keeps_its_milliseconds() {
    let json = bson_to_json(&Bson::DateTime(BsonDateTime::from_millis(
        1_700_000_000_000,
    )));
    assert_eq!(json["$value"], json!("2023-11-14T22:13:20.000Z"));
}

/// A plain JSON object is a subdocument; only the `$type`/`$value` pair means a typed scalar.
#[test]
fn an_object_without_a_type_tag_is_a_subdocument() {
    let parsed = json_to_bson(&json!({ "name": "a", "nested": { "n": 1 } })).unwrap();
    assert_eq!(
        parsed,
        Bson::Document(doc! { "name": "a", "nested": { "n": 1_i32 } })
    );
}

/// Two types can be displayed but not reconstructed, and saying so is better than writing
/// something else in their place.
#[test]
fn the_read_only_types_are_refused_on_the_way_back() {
    assert!(json_to_bson(&json!({ "$type": "DbPointer", "$value": "x" })).is_err());
    assert!(json_to_bson(&json!({ "$type": "JavaScriptWithScope", "$value": "x" })).is_err());
    assert!(json_to_bson(&json!({ "$type": "Nonsense", "$value": "x" })).is_err());
    assert!(json_to_bson(&json!({ "$type": "ObjectId", "$value": "not-hex" })).is_err());
}
