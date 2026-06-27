// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2026, Rémi Bardon <remi@remibardon.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

use arrow_array::{RecordBatch, StringArray};
use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;
use std::fs::File;
use std::path::Path;

pub fn iter_shard<Item: HuggingfaceItem>(path: impl AsRef<Path>) -> impl Iterator<Item = Item> {
    let file = File::open(path).unwrap();

    // Parquet reads one row group at a time from disk; batch_size controls
    // how many rows are decoded into an Arrow RecordBatch per iteration.
    let reader = ParquetRecordBatchReaderBuilder::try_new(file)
        .unwrap()
        .with_batch_size(512)
        .build()
        .unwrap();

    reader.map(Result::unwrap).flat_map(|batch| {
        // Collect into a Vec so we can return an owned iterator from the closure
        extract_articles(&batch)
    })
}

pub trait HuggingfaceItem: for<'b, 'c> From<(&'c Self::Cols<'b>, usize)> {
    type Cols<'b>;

    fn cols<'b>(batch: &'b RecordBatch) -> Option<Self::Cols<'b>>;
}

fn extract_articles<Item: HuggingfaceItem>(batch: &RecordBatch) -> Vec<Item> {
    // Downcast each column once per batch, not once per row
    let Some(cols) = Item::cols(batch) else {
        return vec![];
    };

    (0..batch.num_rows())
        .map(|i| Item::from((&cols, i)))
        .collect()
}

pub(super) fn str_col<'a>(batch: &'a RecordBatch, name: &str) -> Option<&'a StringArray> {
    batch
        .column_by_name(name)?
        .as_any()
        .downcast_ref::<StringArray>()
}
