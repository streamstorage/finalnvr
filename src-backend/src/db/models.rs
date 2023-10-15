use crate::db::schema::cameras;
use diesel::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Queryable, Selectable, Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
#[diesel(table_name = cameras)]
#[diesel(check_for_backend(diesel::sqlite::Sqlite))]
#[serde(rename_all = "camelCase")]
pub struct Camera {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub location: String,
    #[serde(default)]
    pub url: String,
}

#[derive(Insertable)]
#[diesel(table_name = cameras)]
pub struct NewCamera<'a> {
    pub id: &'a str,
    pub name: &'a str,
    pub location: &'a str,
    pub url: &'a str,
}
