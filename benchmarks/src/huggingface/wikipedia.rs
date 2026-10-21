// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2026, Rémi Bardon <remi@remibardon.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

use arrow_array::StringArray;

use super::load::{HuggingfaceItem, str_col};

#[allow(dead_code)]
pub struct WikipediaArticle {
    pub id: String,
    pub title: String,
    pub url: String,
    pub text: String,
}

pub struct WikipediaArticleCols<'a> {
    ids: &'a StringArray,
    titles: &'a StringArray,
    urls: &'a StringArray,
    texts: &'a StringArray,
}

impl HuggingfaceItem for WikipediaArticle {
    type Cols<'b> = WikipediaArticleCols<'b>;

    fn cols<'b>(batch: &'b arrow_array::RecordBatch) -> Option<Self::Cols<'b>> {
        let ids = str_col(batch, "id")?;
        let titles = str_col(batch, "title")?;
        let urls = str_col(batch, "url")?;
        let texts = str_col(batch, "text")?;

        Some(WikipediaArticleCols {
            ids,
            titles,
            urls,
            texts,
        })
    }
}

impl<'b, 'c> From<(&'c WikipediaArticleCols<'b>, usize)> for WikipediaArticle {
    fn from((cols, i): (&'c WikipediaArticleCols<'b>, usize)) -> Self {
        WikipediaArticle {
            id: cols.ids.value(i).to_owned(),
            title: cols.titles.value(i).to_owned(),
            url: cols.urls.value(i).to_owned(),
            text: cols.texts.value(i).to_owned(),
        }
    }
}
