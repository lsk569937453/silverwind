use uuid::Uuid;

pub fn get_uuid() -> String {
    let id = Uuid::new_v4();
    id.to_string()
}
