#[cfg(test)]
use diesel::debug_query;
use diesel::insert_into;
#[cfg(test)]
use diesel::mysql::Mysql;
use diesel::prelude::*;
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

pub fn insert_tuple_batch_with_default(conn: &mut MysqlConnection) -> QueryResult<usize> {
    use schema::users::dsl::*;

    insert_into(users)
        .values(&vec![
            (name.eq("Sean"), Some(hair_color.eq("Black"))),
            (name.eq("Ruby"), None),
        ])
        .execute(conn)
}
