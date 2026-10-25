//! iced's `table(columns, rows)` and `table::column(header, view)`.
//!
//! Each cell is its column's view of its row, made when the table is drawn.

use std::cell::RefCell;

use iced::widget::Widget as _;
use rawr::host;
use rawr::Dynamic;

use super::builtin::{
    alignment, arity, length, pixels, project, runtime_error, take, witness_error, Witnessed,
};
use crate::{marker, runtime, widget};

pub(super) fn bind(builder: &mut host::runtime::Builder) {
    builder
        .bind_type_attestation("iced::widget::table::Column", |value, expected| {
            let value = value.read_lock::<marker::TableColumn>()?;
            (expected.argument_count() == 2)
                .then(|| expected.attest_witness(Some(&value.witness)))
                .flatten()
        })
        .expect("the iced facade must declare generic widget::table::Column exactly once");
    builder
        .bind_type_attestation("iced::widget::Table", |value, expected| {
            let value = value.read_lock::<marker::Table>()?;
            (expected.argument_count() == 1)
                .then(|| expected.attest_witness(Some(&value.witness)))
                .flatten()
        })
        .expect("the iced facade must declare generic widget::Table exactly once");
    builder
        .bind_projection_with_context("iced::widget::Table", "iced::Element", |call, args| {
            project::<marker::Table>(call, args, "iced::widget::Table")
        })
        .expect("the iced facade must retain the Table to Element projection");
    builder
        .bind_with_context("iced::widget::table::column", |call, args| {
            let [header, view] = args else {
                return Err(arity("table::column", 2, args.len()));
            };
            Ok(Dynamic::from(marker::TableColumn {
                witness: call.output_witness().map_err(witness_error)?,
                header: take::<widget::Value>(header, "iced::Element")?,
                view: take::<rawr::item::Value>(view, "table column view")?,
                width: None,
                align_x: None,
                align_y: None,
            }))
        })
        .expect("the iced facade must retain table::column");
    builder
        .bind_with_context("iced::widget::table", |call, args| {
            let [columns, rows] = args else {
                return Err(arity("table", 2, args.len()));
            };
            let columns = columns
                .take()
                .into_typed_array::<Dynamic>()
                .map_err(|_| runtime_error("table expects a list of columns".into()))?
                .into_iter()
                .map(|mut column| {
                    take::<marker::TableColumn>(&mut column, "iced::widget::table::Column")
                })
                .collect::<Result<Vec<_>, _>>()?;
            let rows = rows
                .take()
                .into_typed_array::<Dynamic>()
                .map_err(|_| runtime_error("table expects a list of rows".into()))?;
            Ok(Dynamic::from(marker::Table {
                witness: call.output_witness().map_err(witness_error)?,
                identity: widget::Identity::unique(),
                columns,
                rows,
                width: None,
                padding_x: None,
                padding_y: None,
                separator_x: None,
                separator_y: None,
            }))
        })
        .expect("the iced facade must retain table");
    for name in ["width", "align_x", "align_y"] {
        let path = format!("iced::widget::table::Column::{name}");
        builder
            .bind(path.clone(), move |args| {
                let [column, value] = args else {
                    return Err(arity(&path, 2, args.len()));
                };
                let mut column =
                    take::<marker::TableColumn>(column, "iced::widget::table::Column")?;
                match name {
                    "width" => column.width = Some(length(value)?),
                    "align_x" => column.align_x = Some(alignment(value)?.into()),
                    _ => column.align_y = Some(alignment(value)?.into()),
                }
                Ok(Dynamic::from(column))
            })
            .expect("the iced facade must retain every table::Column builder");
    }
    for name in [
        "width",
        "padding",
        "padding_x",
        "padding_y",
        "separator",
        "separator_x",
        "separator_y",
    ] {
        let path = format!("iced::widget::Table::{name}");
        builder
            .bind(path.clone(), move |args| {
                let [table, value] = args else {
                    return Err(arity(&path, 2, args.len()));
                };
                let mut table = take::<marker::Table>(table, "iced::widget::Table")?;
                if name == "width" {
                    table.width = Some(length(value)?);
                    return Ok(Dynamic::from(table));
                }
                let value = Some(pixels(value, &path)?);
                match name {
                    "padding" => (table.padding_x, table.padding_y) = (value, value),
                    "padding_x" => table.padding_x = value,
                    "padding_y" => table.padding_y = value,
                    "separator" => (table.separator_x, table.separator_y) = (value, value),
                    "separator_x" => table.separator_x = value,
                    _ => table.separator_y = value,
                }
                Ok(Dynamic::from(table))
            })
            .expect("the iced facade must retain every Table builder");
    }
}

impl widget::Descriptor for marker::Table {
    fn lower<'a>(&self, context: &widget::Context<'a>) -> iced::Element<'a, runtime::Message> {
        let mut columns = Vec::with_capacity(self.columns.len());
        for column in &self.columns {
            let mut cells = Vec::with_capacity(self.rows.len());
            for row in &self.rows {
                let cell = match context.invoke(&column.view, &mut [row.clone()]) {
                    None => iced::widget::space().boxed(),
                    Some(Ok(mut cell)) => match take::<widget::Value>(&mut cell, "iced::Element") {
                        Ok(cell) => cell.lower(context),
                        Err(error) => failed(&error.to_string(), context),
                    },
                    Some(Err(error)) => failed(&error, context),
                };
                cells.push(Some(cell));
            }
            let cells = RefCell::new(cells);
            let header = iced::widget::container(column.header.lower(context));
            let mut native = iced::widget::table::column(header, move |row: usize| {
                let cell = cells
                    .borrow_mut()
                    .get_mut(row)
                    .and_then(Option::take)
                    .unwrap_or_else(|| iced::widget::space().boxed());
                iced::widget::container(cell)
            });
            if let Some(width) = column.width {
                native = native.width(width);
            }
            if let Some(align_x) = column.align_x {
                native = native.align_x(align_x);
            }
            if let Some(align_y) = column.align_y {
                native = native.align_y(align_y);
            }
            columns.push(native);
        }
        let mut table = iced::widget::table(columns, 0..self.rows.len());
        if let Some(width) = self.width {
            table = table.width(width);
        }
        if let Some(padding) = self.padding_x {
            table = table.padding_x(padding);
        }
        if let Some(padding) = self.padding_y {
            table = table.padding_y(padding);
        }
        if let Some(separator) = self.separator_x {
            table = table.separator_x(separator);
        }
        if let Some(separator) = self.separator_y {
            table = table.separator_y(separator);
        }
        table.boxed()
    }

    fn identity(&self) -> widget::Identity {
        self.identity.clone()
    }
}

impl Witnessed for marker::Table {
    fn witness(&self) -> &host::runtime::Witness {
        &self.witness
    }
}

fn failed<'a>(error: &str, context: &widget::Context<'a>) -> iced::Element<'a, runtime::Message> {
    widget::diagnostic(format!("table cell failed: {error}")).lower(context)
}
