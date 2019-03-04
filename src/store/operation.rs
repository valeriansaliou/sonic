// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2019, Valerian Saliou <valerian@valeriansaliou.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

use crate::query::query::Query;

pub struct StoreOperationDispatch;

impl StoreOperationDispatch {
    pub fn dispatch(query: Query) -> Result<Option<String>, ()> {
        match query {
            Query::Search(_, _, _, _, _) => {
                // TODO
                Ok(Some(
                    vec![
                        "session_71f3d63b-57c4-40fb-8557-e11309170edd",
                        "session_6501e83a-b778-474f-b60c-7bcad54d755f",
                        "session_8ab1dcdd-eb53-4294-a7d1-080a7245622d",
                    ]
                    .join(" "),
                ))
            }
            Query::Push(_, _) => {
                // TODO
                Ok(None)
            }
            Query::Pop(_) => {
                // TODO
                Ok(Some("0".to_string()))
            }
            Query::Count(_) => {
                // TODO
                Ok(Some("0".to_string()))
            }
            Query::FlushC(_) => {
                // TODO
                Ok(Some("0".to_string()))
            }
            Query::FlushB(_) => {
                // TODO
                Ok(Some("0".to_string()))
            }
            Query::FlushO(_) => {
                // TODO
                Ok(Some("0".to_string()))
            }
        }
    }
}
