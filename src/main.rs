use rocket::fs::{FileServer, NamedFile};
use rocket_db_pools::Database;

#[macro_use]
extern crate rocket;

mod api {

    use std::path::PathBuf;
    use std::str::FromStr;

    use rocket::form::Form;
    use rocket::futures::TryFutureExt;
    use rocket::response::Redirect;
    use rocket::{futures::StreamExt, serde::json::Json};
    use rocket_db_pools::sqlx::sqlite::SqliteRow;
    use rocket_db_pools::{
        sqlx::{Row, SqlitePool},
        Connection, Database,
    };
    use serde::{Deserialize, Serialize};

    #[derive(Database)]
    #[database("db")]
    pub(super) struct RoboDatabase(SqlitePool);

    #[derive(Serialize, Deserialize)]
    struct Product {
        name: String,
        desc: String,
        image: Option<PathBuf>,
    }
    impl TryFrom<SqliteRow> for Product {
        type Error = String;

        fn try_from(value: SqliteRow) -> Result<Self, Self::Error> {
            Ok(Self {
                name: value
                    .try_get("name")
                    .map_err(|e| format!("Could not get `name` {e}"))?,
                desc: value
                    .try_get("desc")
                    .map_err(|e| format!("Could not get `desc` {e}"))?,
                image: match value.try_get("image") {
                    // If there is an error in the path, it treats it as if it was missing
                    Ok(s) => PathBuf::from_str(s).ok(),
                    Err(e) => match e {
                        rocket_db_pools::sqlx::Error::ColumnDecode { .. } => None,
                        _ => return Err(format!("Could not get `image` {e}")),
                    },
                },
            })
        }
    }
    #[derive(Serialize, Deserialize)]
    enum VarTag {
        Size(String),
        Color(String),
        // For hat
        Fitted(bool),
    }
    impl FromStr for VarTag {
        type Err = ();

        fn from_str(s: &str) -> Result<Self, Self::Err> {
            if s.starts_with("size") {
                Ok(VarTag::Size(s[4..].to_owned()))
            } else if s.starts_with("color") {
                Ok(VarTag::Color(s[5..].to_owned()))
            } else if s == "fitted" {
                Ok(VarTag::Fitted(true))
            } else if s == "snap" {
                Ok(VarTag::Fitted(false))
            } else {
                eprintln!("Couldnt match `{s}`");
                Err(())
            }
        }
    }
    #[derive(Serialize, Deserialize)]
    struct ProductVariant {
        quantity: Option<u32>,
        tags: Vec<VarTag>,
    }
    impl TryFrom<SqliteRow> for ProductVariant {
        type Error = String;

        fn try_from(value: SqliteRow) -> Result<Self, Self::Error> {
            Ok(Self {
                quantity: value
                    .try_get::<Option<u32>, _>("quantity")
                    .map_err(|e| format!("Could not get `quantity` {e}"))?,
                tags: value
                    .try_get::<String, _>("tag_name")
                    .map_err(|e| format!("Could not get `tag_name` {e}"))?
                    .split_whitespace()
                    .filter_map(|s| s.parse().ok())
                    .collect(),
                // name: value
                //     .try_get("name")
                //     .map_err(|e| format!("Could not get `name` {e}"))?,
            })
        }
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
    pub(super) async fn get_items(
        mut db: Connection<RoboDatabase>,
    ) -> Result<Json<Vec<Product>>, String> {
        let rows: Vec<Result<Product, String>> =
            rocket_db_pools::sqlx::query("select * from products")
                .fetch(&mut **db)
                .map(|row| {
                    let row = row.map_err(|e| format!("Couldnt get row {e}"))?;
                    let item_r: Result<Product, String> = row.try_into();
                    item_r
                })
                .collect()
                .await;
        let mut rows_ret = Vec::with_capacity(rows.len());
        for row in rows {
            match row {
                Ok(row) => rows_ret.push(row),
                Err(e) => return Err(e),
            }
        }
        Ok(Json(rows_ret))
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
    #[post("/getvariants", data = "<name>")]
    pub(super) async fn get_product_variants(
        name: &str,
        mut db: Connection<RoboDatabase>,
    ) -> Result<Json<Vec<ProductVariant>>, String> {
        let product_id: u32 =
            rocket_db_pools::sqlx::query("select product_id from products where name = $1")
                .bind(name)
                .fetch_one(&mut **db)
                .map_err(|e| format!("Could not find product {e}"))
                .await?
                .try_get("product_id")
                .map_err(|e| format!("Could not get product_id from product {e}"))?;
        let prod_vars: Vec<Result<ProductVariant, String>> =
            rocket_db_pools::sqlx::query("select * from product_variants where product_id = $1")
                .bind(product_id)
                .fetch(&mut **db)
                .map(|row| {
                    let row = match row {
                        Ok(row) => row,
                        Err(e) => return Err(format!("Row in product variants not found {e}")),
                    };
                    row.try_into()
                })
                .collect()
                .await;
        let mut prod_vars_ret = vec![];
        for prodvar in prod_vars {
            match prodvar {
                Ok(r) => prod_vars_ret.push(r),
                Err(e) => return Err(e),
            }
        }
        Ok(Json(prod_vars_ret))
    }
    #[allow(private_interfaces)]
    #[get("/get_websiteinfo")]
    pub(super) async fn get_websiteinfo(mut db: Connection<RoboDatabase>) -> Json<Vec<Product>> {
        let rows = rocket_db_pools::sqlx::query("select * from website_information")
            .fetch(&mut **db)
            .filter_map(|row| async move {
                let row = row.ok()?;
                Some(Product {
                    name: row.try_get("name").ok()?,
                    desc: row.try_get("desc").ok()?,
                    image: None,
                })
            })
            .collect()
            .await;
        Json(rows)
    }
    #[allow(private_interfaces)]
    #[post("/update_websiteinfo", data = "<info>")]
    pub(super) async fn update_websiteinfo(
        info: Form<WebsiteInfo>,
        mut db: Connection<RoboDatabase>,
    ) -> Result<Redirect, String> {
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
        .mount("/", routes![homepage])
        .mount("/", FileServer::from("./pages"))
        .mount(
            "/api",
            routes![
                api::get_items,
                api::add_item,
                api::get_websiteinfo,
                api::update_websiteinfo,
                api::get_product_variants
            ],
        )
}
