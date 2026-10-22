use alloc::format;
use alloc::string;
use alloc::vec;
use alloc::vec::Vec;
use core::cell::RefCell;

use crate::decoder::StringBuilder as _;
use crate::parser::{Event, EventKind, EventReceiver as _};
use crate::Span;

use super::storage::{Buffer, Builder, Session, Text};
use super::{
    parse, syntax, Admission, Array, Diagnostic, Document, Error, Integer, Key, Request, Spanned,
    Table, Tag, Value,
};

#[derive(Clone, Debug)]
struct Account {
    items: usize,
    bytes: usize,
    item_limit: usize,
    byte_limit: usize,
    string_limit: usize,
    requests: Vec<Request>,
    refuse_at: Option<usize>,
}

impl Default for Account {
    fn default() -> Self {
        Self {
            items: 17,
            bytes: 53,
            item_limit: usize::MAX,
            byte_limit: usize::MAX,
            string_limit: usize::MAX,
            requests: Vec::new(),
            refuse_at: None,
        }
    }
}

impl Admission for Account {
    type Error = &'static str;

    fn admit(&mut self, request: Request) -> Result<(), Self::Error> {
        let index = self.requests.len();
        self.requests.push(request);
        if self.refuse_at == Some(index) {
            return Err("refused");
        }
        match request {
            Request::Items(count) => {
                let next = self.items.checked_add(count).ok_or("items")?;
                if next > self.item_limit {
                    return Err("items");
                }
                self.items = next;
            }
            Request::Allocation { bytes, alignment } => {
                assert!(alignment.is_power_of_two());
                let next = self.bytes.checked_add(bytes).ok_or("bytes")?;
                if next > self.byte_limit {
                    return Err("bytes");
                }
                self.bytes = next;
            }
            Request::String(bytes) => {
                if bytes > self.string_limit {
                    return Err("string");
                }
            }
        }
        Ok(())
    }
}

fn parsed(input: &str) -> Document<'_> {
    parse(input, 80, &mut Account::default()).expect("valid pinned TOML")
}

fn invalid(input: &str) -> Diagnostic {
    match parse(input, 80, &mut Account::default()) {
        Err(Error::Invalid(error)) => error,
        other => panic!("expected invalid TOML: {other:?}"),
    }
}

fn table<'a, 'i>(value: &'a Spanned<Value<'i>>) -> &'a Table<'i> {
    match value.value() {
        Value::Table(table) => table,
        _ => panic!("table"),
    }
}

fn array<'a, 'i>(value: &'a Spanned<Value<'i>>) -> &'a Array<'i> {
    match value.value() {
        Value::Array(array) => array,
        _ => panic!("array"),
    }
}

fn integer<'a, 'i>(value: &'a Spanned<Value<'i>>) -> &'a Integer<'i> {
    match value.value() {
        Value::Integer(integer) => integer,
        _ => panic!("integer"),
    }
}

#[test]
fn source_borrow_and_full_document() {
    let input = "title = 'demo'\n[unused]\nvalue = [1, {x=true}]\n";
    let document = parsed(input);
    assert!(core::ptr::eq(document.source().as_ptr(), input.as_ptr()));
    assert_eq!(document.table().len(), 2);
    assert_eq!(table(document.table().get("unused").unwrap()).len(), 1);
}

#[test]
fn implicit_table_shift_remove_reinsert_changes_order() {
    let document = parsed("[a.b]\nx=1\n[z]\ny=2\n[a]\nq=3\n");
    let keys: Vec<_> = document
        .table()
        .iter()
        .map(|(key, _)| key.as_str())
        .collect();
    assert_eq!(keys, ["z", "a"]);
    let a = table(document.table().get("a").unwrap());
    assert!(!a.is_implicit());
    assert!(!a.is_dotted());
    assert_eq!(
        a.iter().map(|(key, _)| key.as_str()).collect::<Vec<_>>(),
        ["b", "q"]
    );
}

#[test]
fn decoded_escaped_keys_collide_with_context() {
    let error = invalid("a = 1\n\"\\u0061\" = 2\n");
    assert_eq!(error.tag, Tag::DuplicateKey);
    assert_eq!(error.context, Some(0..1));
    assert_eq!(error.span, Some(6..14));
}

#[test]
fn dotted_inline_flags_and_collision() {
    let document = parsed("x = {a.b=1, a.c=2}\n");
    let x = table(document.table().get("x").unwrap());
    let a = table(x.get("a").unwrap());
    assert!(x.is_inline());
    assert!(a.is_inline() && a.is_implicit() && a.is_dotted());
    assert_eq!(a.len(), 2);
    assert_eq!(invalid("x = {a={b=1}, a.c=2}\n").tag, Tag::DuplicateKey);
    assert_eq!(invalid("x = {a.b=1, a={c=2}}\n").tag, Tag::DuplicateKey);
}

#[test]
fn explicit_dotted_and_inline_extension_rules() {
    assert_eq!(invalid("a.b=1\n[a]\nc=2\n").tag, Tag::DuplicateKey);
    assert_eq!(invalid("a={b=1}\n[a.c]\nd=2\n").tag, Tag::ExtendInlineTable);
    assert_eq!(invalid("a=[]\n[a.b]\nx=1\n").tag, Tag::ExtendWrongType);
    assert_eq!(invalid("[a]\nx=1\n[a]\ny=2\n").tag, Tag::DuplicateKey);
}

#[test]
fn array_table_descent_selects_latest_instance() {
    let document = parsed("[[a]]\nn=1\n[a.child]\nx=2\n[[a]]\nn=3\n[a.child]\nx=4\n");
    let a = array(document.table().get("a").unwrap());
    assert!(a.is_array_of_tables());
    assert_eq!(a.values().len(), 2);
    let first = table(&a.values()[0]);
    let last = table(&a.values()[1]);
    assert_eq!(integer(first.get("n").unwrap()).to_i64(), Ok(1));
    assert_eq!(integer(last.get("n").unwrap()).to_i64(), Ok(3));
    assert_eq!(
        integer(table(first.get("child").unwrap()).get("x").unwrap()).to_i64(),
        Ok(2)
    );
    assert_eq!(
        integer(table(last.get("child").unwrap()).get("x").unwrap()).to_i64(),
        Ok(4)
    );
    assert_eq!(invalid("a=[{}]\n[[a]]\nx=1\n").tag, Tag::DuplicateKey);
}

#[test]
fn scalar_spelling_and_conversion_stay_deferred() {
    let document =
        parsed("hex=0xD_EAD\nhuge=999999999999999999999999999999999\nf=1e9999\ni=+inf\n");
    let hex = integer(document.table().get("hex").unwrap());
    assert_eq!((hex.as_str(), hex.radix()), ("DEAD", 16));
    assert!(integer(document.table().get("huge").unwrap())
        .to_i64()
        .is_err());
    let Value::Float(float) = document.table().get("f").unwrap().value() else {
        panic!("float")
    };
    assert_eq!(float.as_str(), "1e9999");
    assert_eq!(float.to_f64(), None);
    let Value::Float(infinity) = document.table().get("i").unwrap().value() else {
        panic!("float")
    };
    assert!(infinity.to_f64().unwrap().is_infinite());
}

#[test]
fn datetime_validation_and_error_precedence() {
    let document = parsed("date=2024-02-29\ntime=07:32:00\nstamp=1979-05-27T07:32:00Z\n");
    assert!(matches!(
        document.table().get("date").unwrap().value(),
        Value::DateTime(_)
    ));
    assert_eq!(invalid("date=2023-02-29\n").tag, Tag::DateTime);
    assert_eq!(invalid("a=1\na=2023-02-29\n").tag, Tag::DateTime);
    assert_eq!(invalid("a=1\na=2\nz=2023-02-29\n").tag, Tag::DuplicateKey);
    assert_eq!(invalid("a=1\na=2\nz=[\n").tag, Tag::Syntax);
}

#[test]
fn ignored_values_and_joined_raw_scalar_are_fully_decoded() {
    assert_eq!(invalid("name='valid'\nunused=[1 2]\n").tag, Tag::Syntax);
    assert_eq!(invalid("name='valid'\nunused=\"\\q\"\n").tag, Tag::Syntax);
    assert_eq!(
        invalid("name='valid'\nunused={x=1,x=2}\n").tag,
        Tag::DuplicateKey
    );
}

#[test]
fn complete_string_forms_and_decoded_expansion() {
    let document = parsed(
        "a=\"\\u00e9\\U0001F600\"\nb='''\nraw\\value\n'''\nc=\"\"\"\nhello\\\n   world\"\"\"\n",
    );
    let text = |key| match document.table().get(key).unwrap().value() {
        Value::String(value) => value.as_str(),
        _ => panic!("string"),
    };
    assert_eq!(text("a"), "é😀");
    assert_eq!(text("b"), "raw\\value\n");
    assert_eq!(text("c"), "helloworld");
    let mut account = Account {
        string_limit: 3,
        ..Account::default()
    };
    assert!(matches!(
        parse("a=\"\\U0001F600\"", 80, &mut account),
        Err(Error::Admission("string"))
    ));
}

#[test]
fn exact_and_floor_cumulative_accounts_keep_prior_usage() {
    let input = "[a.b]\nname=\"a\\u0062c\"\nvalues=[1,2,3,4,5]\n[z]\nx=true\n[a]\ny=2\n";
    let mut measured = Account::default();
    parse(input, 80, &mut measured).unwrap();
    let mut exact = Account {
        item_limit: measured.items,
        byte_limit: measured.bytes,
        ..Account::default()
    };
    parse(input, 80, &mut exact).unwrap();
    assert_eq!((exact.items, exact.bytes), (measured.items, measured.bytes));
    assert_eq!(exact.requests, measured.requests);
    let mut byte_floor = Account {
        byte_limit: measured.bytes - 1,
        ..Account::default()
    };
    assert!(matches!(
        parse(input, 80, &mut byte_floor),
        Err(Error::Admission("bytes"))
    ));
    let mut item_floor = Account {
        item_limit: measured.items - 1,
        ..Account::default()
    };
    assert!(matches!(
        parse(input, 80, &mut item_floor),
        Err(Error::Admission("items"))
    ));
}

#[test]
fn every_admission_refusal_is_permanent() {
    let input = "[\"a\\u0062\".child]\nx={a.b=[\"\\u00e9\",2]}\n[ab]\ny=1\n";
    let mut measured = Account::default();
    parse(input, 80, &mut measured).unwrap();
    for refuse_at in 0..measured.requests.len() {
        let mut account = Account {
            refuse_at: Some(refuse_at),
            ..Account::default()
        };
        assert!(matches!(
            parse(input, 80, &mut account),
            Err(Error::Admission("refused"))
        ));
        assert_eq!(account.requests.len(), refuse_at + 1);
    }
}

#[test]
fn full_replacement_layouts_precede_buffer_growth() {
    let mut account = Account::default();
    let session = RefCell::new(Session::new(&mut account));
    let mut buffer = Buffer::new();
    for number in 0u64..9 {
        assert!(buffer.push(number, &session));
    }
    let layouts: Vec<_> = account
        .requests
        .iter()
        .filter_map(|request| match request {
            Request::Allocation { bytes, alignment } => Some((*bytes, *alignment)),
            _ => None,
        })
        .collect();
    assert_eq!(layouts, [(32, 8), (64, 8), (128, 8)]);
    assert_eq!(account.bytes, 53 + 32 + 64 + 128);
}

#[test]
fn builder_clear_cannot_reset_refusal() {
    let mut account = Account {
        string_limit: 1,
        ..Account::default()
    };
    let session = RefCell::new(Session::new(&mut account));
    let mut builder = Builder::new(&session);
    assert!(builder.push_str("a"));
    assert!(!builder.push_char('é'));
    builder.clear();
    assert!(!builder.push_str("x"));
    assert!(!builder.push_char('x'));
    assert!(matches!(
        session.borrow().failure,
        Some(Error::Admission("string"))
    ));
}

#[test]
fn open_callbacks_refuse_after_latched_failure() {
    let mut account = Account {
        refuse_at: Some(0),
        ..Account::default()
    };
    let session = RefCell::new(Session::new(&mut account));
    let mut events = Buffer::<Event>::new();
    let mut receiver = syntax::Receiver {
        events: &mut events,
        session: &session,
    };
    let span = Span::new_unchecked(0, 1);
    let mut errors = |_error: crate::ParseError| {};
    assert!(!receiver.inline_table_open(span, &mut errors));
    assert!(!receiver.array_open(span, &mut errors));
    receiver.error(span, &mut errors);
    assert_eq!(events.len(), 0);
    assert_eq!(account.requests.len(), 1);
}

#[test]
fn malformed_negative_depth_recovery_cannot_enter_deep_arrays() {
    // The unexpected brackets inside an inline table produce closes without
    // matching RecursionGuard opens in the pinned recovery control flow.
    let mut input = string::String::from("x={");
    for _ in 0..200 {
        input.push_str("[] ");
    }
    input.push_str("}\ny=");
    input.push_str(&"[".repeat(10_000));
    input.push('0');
    input.push_str(&"]".repeat(10_000));
    let mut measured = Account::default();
    assert!(matches!(
        parse(&input, 80, &mut measured),
        Err(Error::Invalid(_))
    ));
    // First diagnostic is the final admission request, and refusing that
    // callback must still close subsequent container admission permanently.
    let mut refused = Account {
        refuse_at: Some(measured.requests.len() - 1),
        ..Account::default()
    };
    assert!(matches!(
        parse(&input, 80, &mut refused),
        Err(Error::Admission("refused"))
    ));
    assert_eq!(refused.requests.len(), measured.requests.len());
}

#[test]
fn raw_events_are_charged_beyond_token_capacity_assumptions() {
    let mut account = Account::default();
    let session = RefCell::new(Session::new(&mut account));
    let mut events = Buffer::new();
    let mut receiver = syntax::Receiver {
        events: &mut events,
        session: &session,
    };
    let span = Span::new_unchecked(0, 0);
    let mut errors = |_error: crate::ParseError| {};
    for _ in 0..17 {
        receiver.error(span, &mut errors);
    }
    assert_eq!(events.len(), 17);
    assert!(events
        .as_slice()
        .iter()
        .all(|event| event.kind() == EventKind::Error));
    assert_eq!(account.items, 17 + 17);
}

#[test]
fn caller_and_pinned_depth_apply_to_containers_and_dotted_keys() {
    assert!(parse("a=[[1]]", 2, &mut Account::default()).is_ok());
    assert!(matches!(
        parse("a=[[[1]]]", 2, &mut Account::default()),
        Err(Error::Invalid(Diagnostic {
            tag: Tag::Depth,
            ..
        }))
    ));
    assert!(parse("a.b=1", 2, &mut Account::default()).is_ok());
    assert!(matches!(
        parse("a.b.c=1", 2, &mut Account::default()),
        Err(Error::Invalid(Diagnostic {
            tag: Tag::Depth,
            ..
        }))
    ));
    let input = format!("a={}1{}", "[".repeat(81), "]".repeat(81));
    assert!(matches!(
        parse(&input, 999, &mut Account::default()),
        Err(Error::Invalid(Diagnostic {
            tag: Tag::Depth,
            ..
        }))
    ));
}

#[test]
fn avl_rotations_removal_replacement_and_order_are_actual_operations() {
    let mut account = Account::default();
    let session = RefCell::new(Session::new(&mut account));
    let keys: Vec<_> = (0..257).map(|index| format!("key{index:03}")).collect();
    let mut table = Table::default();
    let order: Vec<_> = (0..257).map(|index| (index * 131) % 257).collect();
    for &index in &order {
        table
            .insert(
                Key {
                    text: Text::Borrowed(&keys[index]),
                    span: index..index + 1,
                },
                Spanned {
                    value: Value::Boolean(true),
                    span: 0..0,
                },
                &session,
            )
            .unwrap();
        table.check_avl();
    }
    assert_eq!(
        table
            .iter()
            .map(|(key, _)| key.as_str())
            .collect::<Vec<_>>(),
        order
            .iter()
            .map(|&index| keys[index].as_str())
            .collect::<Vec<_>>()
    );
    for &index in order.iter().step_by(2) {
        let (key, value) = table.remove(&keys[index]).unwrap();
        table.check_avl();
        table.insert(key, value, &session).unwrap();
        table.check_avl();
        assert_eq!(table.iter().last().unwrap().0.as_str(), keys[index]);
    }
    let original = table
        .iter()
        .map(|(key, _)| key.as_str())
        .collect::<Vec<_>>();
    let original: Vec<_> = original.into_iter().map(string::String::from).collect();
    let index = order[0];
    table
        .insert(
            Key {
                text: Text::Borrowed(&keys[index]),
                span: 999..1000,
            },
            Spanned {
                value: Value::Boolean(false),
                span: 0..0,
            },
            &session,
        )
        .unwrap();
    assert_eq!(
        table
            .iter()
            .map(|(key, _)| key.as_str())
            .collect::<Vec<_>>(),
        original
    );
    assert_ne!(
        table.entry(table.find(&keys[index]).unwrap()).0.span(),
        999..1000
    );
    for &index in order.iter().rev() {
        table.remove(&keys[index]).unwrap();
        table.check_avl();
    }
    assert!(table.is_empty());
    drop(table);
}

#[test]
fn compounded_dotted_inline_depth_drops_without_recursive_ownership() {
    let dotted = vec!["k"; 80].join(".");
    let mut input = string::String::from("root=");
    for _ in 0..80 {
        input.push_str(&format!("{{{dotted}="));
    }
    input.push('1');
    input.push_str(&"}".repeat(80));
    let document = parsed(&input);
    drop(document);
    // Failure after the same partially retained tree exercises teardown too.
    input.push_str("\nroot=2");
    assert_eq!(invalid(&input).tag, Tag::DuplicateKey);
}

#[test]
fn invalid_documents_report_diagnostics_before_exact_event_admission() {
    let mut body = String::new();
    for index in 0..1000 {
        body.push_str(&format!("k{index} = {index}\n"));
    }
    // Depth refusal reaches the error sink during the syntax pass itself.
    let broken = format!("k = {}{}\n{body}", "[".repeat(100), "]".repeat(100));
    let mut probe = Account::default();
    assert!(matches!(
        parse(&broken, 80, &mut probe),
        Err(Error::Invalid(_))
    ));
    let tokens = probe
        .requests
        .iter()
        .find_map(|request| match request {
            Request::Allocation { bytes, .. } => Some(*bytes),
            _ => None,
        })
        .expect("tokens are admitted once");
    let tight = || Account {
        byte_limit: 53 + tokens + 16 * 1024,
        ..Account::default()
    };
    let outcome = parse(&broken, 80, &mut tight());
    assert!(
        matches!(&outcome, Err(Error::Invalid(diagnostic)) if diagnostic.tag == Tag::Depth),
        "{:?}",
        outcome.err()
    );
    assert!(matches!(
        parse(&body, 80, &mut tight()),
        Err(Error::Admission("bytes"))
    ));
}
