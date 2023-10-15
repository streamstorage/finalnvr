// @generated automatically by Diesel CLI.

diesel::table! {
    cameras (id) {
        id -> Text,
        name -> Text,
        location -> Text,
        url -> Text,
    }
}
