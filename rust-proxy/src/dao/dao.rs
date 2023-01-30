
#[cfg(test)]
use diesel::debug_query;
use diesel::insert_into;
#[cfg(test)]
use diesel::pg::Pg;
use diesel::prelude::*;
use std::error::Error;
use std::time::SystemTime;
mod schema {
    diesel::table! {
        users {
            id -> Integer,
            name -> Text,
            hair_color -> Nullable<Text>,
            created_at -> Timestamp,
            updated_at -> Timestamp,
        }
    }
}

use schema::users;

pub fn insert_tuple_batch_with_default(conn: &mut PgConnection) -> QueryResult<usize> {
    use schema::users::dsl::*;

    insert_into(users)
        .values(&vec![
            (name.eq("Sean"), Some(hair_color.eq("Black"))),
            (name.eq("Ruby"), None),
        ])
        .execute(conn)
}