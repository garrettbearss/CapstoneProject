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
    use rocket::form::Form;
    use rocket::response::Redirect;

    #[derive(Database)]
    #[database("db")]
    pub(super) struct RoboDatabase(SqlitePool);

    #[derive(Serialize, Deserialize)]
    struct Item {
        name: String,
        desc: String,
    }

    #[derive(FromForm)]
    struct WebsiteInfo {
        club_desc1: String,
        club_desc2: String,
        club_history: String,
        club_activities: String,
        join_info: String,
        contact_email: String,
        contact_address: String,
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
                    desc: row.try_get("desc").ok()?,
                    //quantity: row.try_get::<u32, _>("quantity").ok()? as usize,
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
    #[allow(private_interfaces)]
    #[get("/get_websiteinfo")]
    pub(super) async fn get_websiteinfo(mut db: Connection<RoboDatabase>) -> Json<Vec<Item>> {
        let rows = rocket_db_pools::sqlx::query("select * from website_information")
            .fetch(&mut **db)
            .filter_map(|row| async move {
                let row = row.ok()?;
                Some(Item {
                    name: row.try_get("name").ok()?,
                    desc: row.try_get("desc").ok()?,
                })
            })
            .collect()
            .await;
        Json(rows)
    }
    #[allow(private_interfaces)]
#[post("/update_websiteinfo", data = "<info>")]
pub(super) async fn update_websiteinfo(info: Form<WebsiteInfo>, mut db: Connection<RoboDatabase>) -> Result<Redirect, String> {
    // SQL query to update the website information in the database
    let result = rocket_db_pools::sqlx::query(
        "UPDATE website_information SET desc = CASE name
            WHEN 'aboutClub1' THEN ?
            WHEN 'aboutClub2' THEN ?
            WHEN 'clubHistory' THEN ?
            WHEN 'clubActivities' THEN ?
            WHEN 'joinClub' THEN ?
            WHEN 'contact_email' THEN ?
            WHEN 'contact_address' THEN ?
            ELSE desc END
        WHERE name IN ('aboutClub1', 'aboutClub2', 'clubHistory', 'clubActivities', 'joinClub', 'contact_email', 'contact_address');"
    )
    .bind(&info.club_desc1) // for 'aboutClub1'
    .bind(&info.club_desc2) // for 'aboutClub2'
    .bind(&info.club_history) // for 'clubHistory'
    .bind(&info.club_activities) // for 'clubActivities'
    .bind(&info.join_info) // for 'joinClub'
    .bind(&info.contact_email) // for 'contact_email'
    .bind(&info.contact_address) // for 'contact_address'
    .execute(&mut **db)
    .await;

    // Handle the result of the database operation
    match result {
        Ok(_) => Ok(Redirect::to("adminconfirm.html")),
        Err(err) => Err(format!("Database error: {err}")),
    }
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
        .mount("/", routes![homepage, api::update_websiteinfo])
        .mount("/", FileServer::from("./pages"))
        .mount("/api", routes![api::get_items, api::add_item, api::get_websiteinfo, api::update_websiteinfo])
}
