use rocket::fs::{FileServer, NamedFile};
use rocket_db_pools::Database;

#[macro_use]
extern crate rocket;

mod api {
    use rocket::{futures::StreamExt, serde::json::Json};
    use rocket_db_pools::{
        sqlx::{Row, SqlitePool},
        Connection, Database,
    };
    use serde::{Deserialize, Serialize};

    #[derive(Database)]
    #[database("db")]
    pub(super) struct RoboDatabase(SqlitePool);

    #[derive(Serialize, Deserialize)]
    struct Item {
        name: String,
        price: f32,
        quantity: usize,
    }

    #[allow(private_interfaces)]
    #[get("/getitems")]
    pub(super) async fn get_items(mut db: Connection<RoboDatabase>) -> Json<Vec<Item>> {
        let rows = rocket_db_pools::sqlx::query("select * from products")
            .fetch(&mut **db)
            .filter_map(|row| async move {
                let row = row.ok()?;
                Some(Item {
                    name: row.try_get("name").ok()?,
                    price: row.try_get("price").ok()?,
                    quantity: row.try_get::<u32, _>("quantity").ok()? as usize,
                })
            })
            .collect()
            .await;
        Json(rows)
    }
    #[get("/additem/<name>")]
    pub(super) async fn add_item(
        name: &str,
        mut db: Connection<RoboDatabase>,
    ) -> Result<&'static str, String> {
        rocket_db_pools::sqlx::query(
            "insert into products (name, price, quantity) values ($1, $2 ,$3)",
        )
        .bind(name)
        .bind(10.0)
        .bind(100)
        .execute(&mut **db)
        .await
        .map_err(|e| format!("Database error {e}"))?;
        Ok("Added")
    }
}

// Route to set homepage.html on run
#[get("/")]
async fn homepage() -> Option<NamedFile> {
    NamedFile::open("./pages/homepage.html").await.ok()
}

#[launch]
async fn rocket() -> _ {
    rocket::build()
        .attach(api::RoboDatabase::init())
        .mount("/", routes![homepage])
        .mount("/", FileServer::from("./pages"))
        .mount("/api", routes![api::get_items, api::add_item])
}
