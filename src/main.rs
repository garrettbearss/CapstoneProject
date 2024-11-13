use rocket::fs::{FileServer, NamedFile};
use rocket_db_pools::Database;

#[macro_use]
extern crate rocket;

mod api {
    use chrono::{Duration, NaiveDate, NaiveDateTime, Utc};
    use rand::{distributions::Alphanumeric, Rng};
    use rocket::form::Form;
    use rocket::futures::TryFutureExt;
    use rocket::http::Cookie;
    use rocket::http::CookieJar;
    use rocket::http::Status;
    use rocket::response::status::Custom;
    use rocket::{futures::StreamExt, serde::json::Json};
    use rocket_db_pools::sqlx::sqlite::SqliteRow;
    use rocket_db_pools::{
        sqlx::{Row, SqlitePool},
        Connection, Database,
    };
    use serde::{Deserialize, Serialize};
    use serde_json::Value;
    use sha2::{Digest, Sha256};
    use std::path::PathBuf;
    use std::str::FromStr;
    use uuid::Uuid;

    #[derive(Database)]
    #[database("db")]
    pub(super) struct RoboDatabase(SqlitePool);

    #[derive(Serialize, Deserialize)]
    struct Description {
        name: String,
        desc: String,
    }

    #[derive(Serialize, Deserialize)]
    struct Product {
        name: String,
        desc: String,
        price: f32,
        image: Option<PathBuf>,
        quantity: f32,
    }
    impl TryFrom<SqliteRow> for Product {
        type Error = String;

        fn try_from(value: SqliteRow) -> Result<Self, Self::Error> {
            Ok(Self {
                name: value
                    .try_get("name")
                    .map_err(|e| format!("Could not get `name`: {e}"))?,
                desc: value
                    .try_get("desc")
                    .map_err(|e| format!("Could not get `desc`: {e}"))?,
                price: value
                    .try_get("price")
                    .map_err(|e| format!("Could not get `price`: {e}"))?, // New field
                image: match value.try_get("image") {
                    // If there is an error in the path, it treats it as if it was missing
                    Ok(s) => PathBuf::from_str(s).ok(),
                    Err(e) => match e {
                        rocket_db_pools::sqlx::Error::ColumnDecode { .. } => None,
                        _ => return Err(format!("Could not get `image`: {e}")),
                    },
                },
                quantity: value
                    .try_get("quantity")
                    .map_err(|e| format!("Could not get `quantity`: {e}"))?,
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
    impl ToString for VarTag {
        fn to_string(&self) -> String {
            match self {
                VarTag::Size(s) => format!("size{s}"),
                VarTag::Color(c) => format!("color{c}"),
                VarTag::Fitted(f) => {
                    if *f {
                        format!("fitted")
                    } else {
                        format!("snap")
                    }
                }
            }
        }
    }
    #[derive(Serialize, Deserialize)]
    struct ProductVariant {
        quantity: Option<u32>,
        tags: Vec<VarTag>,
        product: u32,
        varid: u32,
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
                product: value
                    .try_get("product_id")
                    .map_err(|e| format!("Could not get `product_id` {e}"))?,
                varid: value
                    .try_get("var_id")
                    .map_err(|e| format!("Could not get `var_id` {e}"))?,
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

    #[derive(FromForm)]
    struct CreateAdmin {
        username: String,
        password: String,
        expiration: String,
    }

    #[derive(FromForm)]
    struct LoginCredentials {
        username: String,
        password: String,
    }

    #[derive(FromForm, Serialize, Deserialize)]
    struct AdminUser {
        id: i32,
        username: String,
    }

    #[derive(Serialize)]
    struct CurrentUserResponse {
        username: String,
    }

    #[allow(private_interfaces)]
    #[get("/current_user")]
    pub async fn current_user(
        jar: &CookieJar<'_>,
        mut db: Connection<RoboDatabase>,
    ) -> Option<Json<CurrentUserResponse>> {
        // Get the token from the cookie jar
        if let Some(token_cookie) = jar.get("token") {
            let token_value = token_cookie.value();

            // Query the database to get the username for the token
            if let Ok(row) =
                rocket_db_pools::sqlx::query("SELECT username FROM admins WHERE token = ?")
                    .bind(token_value)
                    .fetch_one(&mut **db)
                    .await
            {
                if let Ok(username) = row.try_get::<String, _>("username") {
                    return Some(Json(CurrentUserResponse { username }));
                }
            }
        }
        None // Return None if no valid user is found
    }

    fn generate_token_and_expiration() -> (String, chrono::DateTime<Utc>) {
        let token = Uuid::new_v4().to_string(); // Generate a unique token
        let expiration = Utc::now() + Duration::minutes(5); // Set expiration time to 5 minutes from now
        (token, expiration) // Return both the token and its expiration time
    }

    #[derive(Serialize)]
    struct ResponseData {
        success: bool,
        message: String,
    }

    #[allow(private_interfaces)]
    #[post("/login", data = "<login_form>")]
    pub async fn login(
        login_form: Form<LoginCredentials>,
        mut db: Connection<RoboDatabase>,
        jar: &CookieJar<'_>,
    ) -> Result<Json<ResponseData>, Custom<Json<ResponseData>>> {
        // Fetch the admin from the database using the provided username
        let row = rocket_db_pools::sqlx::query("SELECT * FROM admins WHERE username = ?")
            .bind(&login_form.username)
            .fetch_one(&mut **db)
            .await
            .map_err(|e| {
                Custom(
                    Status::InternalServerError,
                    Json(ResponseData {
                        success: false,
                        message: format!("Database error: {}", e),
                    }),
                )
            })?;

        let hashed_password = row
            .try_get::<String, _>("password")
            .map_err(|e| Custom(Status::InternalServerError, Json(ResponseData {
                success: false,
                message: format!("Database error: {}", e),
            })))?;
        let salt: String = row.try_get("salt").map_err(|e| Custom(Status::InternalServerError, Json(ResponseData {
            success: false,
            message: format!("Database error: {}", e),
        })))?;
        let expiration_str: String = row.try_get("expiration").map_err(|e| Custom(Status::InternalServerError, Json(ResponseData {
            success: false,
            message: format!("Database error: {}", e),
        })))?;

        // Parse the expiration date from the string in "YYYY-MM-DD" format
        let expiration_date = NaiveDate::parse_from_str(&expiration_str, "%Y-%m-%d")
            .map_err(|e| Custom(Status::BadRequest, Json(ResponseData {
                success: false,
                message: format!("Date parse error: {}", e),
            })))?;

        // Get the current UTC date
        let now = Utc::now().naive_utc().date();

        // Check if the admin's expiration date has passed
        if now > expiration_date {
            // Remove the expired admin account from the database
            rocket_db_pools::sqlx::query("DELETE FROM admins WHERE username = ?")
                .bind(&login_form.username)
                .execute(&mut **db)
                .await
                .map_err(|e| Custom(Status::InternalServerError, Json(ResponseData {
                    success: false,
                    message: format!("Failed to remove expired admin: {}", e),
                })))?;

            return Err(Custom(Status::Unauthorized, Json(ResponseData {
                success: false,
                message: "Admin account has expired and has been removed.".into(),
            })));
        }

        // Combine the input password with the salt and hash it
        let salted_input_password = format!("{}{}", login_form.password, salt);
        let hashed_input_password = hash_password(&salted_input_password);

        // Check the hashed input password against the stored hashed password
        if hashed_input_password == hashed_password {
            // Generate a new token and its expiration
            let (token, expiration) = generate_token_and_expiration();
            let expiration_string = expiration.to_rfc3339();

            // Update the user's token and its expiration in the database
            rocket_db_pools::sqlx::query(
                "UPDATE admins SET token = ?, token_expiration = ? WHERE username = ?",
            )
            .bind(&token)
            .bind(expiration_string)
            .bind(&login_form.username)
            .execute(&mut **db)
            .await
            .map_err(|e| Custom(Status::InternalServerError, Json(ResponseData {
                success: false,
                message: format!("Failed to update token: {}", e),
            })))?;

            // Store the token in a cookie
            jar.add(Cookie::new("token", token));

            Ok(Json(ResponseData {
                success: true,
                message: "Login successful.".into(),
            }))
        } else {
            Err(Custom(Status::Unauthorized, Json(ResponseData {
                success: false,
                message: "Invalid username or password.".into(),
            })))
        }
    }

    fn validate_user(_token: &str) -> Option<String> {
        Some("test".into())
    }

    #[get("/admin_menu")]
    pub async fn admin_menu(
        jar: &CookieJar<'_>,
        mut db: Connection<RoboDatabase>,
    ) -> Result<Json<Value>, String> {
        let token = jar.get("token").map(|c| c.value().to_string());

        if let Some(token_value) = token {
            // Validate the token
            let user = rocket_db_pools::sqlx::query("SELECT * FROM admins WHERE token = ?")
                .bind(&token_value)
                .fetch_one(&mut **db)
                .await;

            if let Ok(row) = user {
                // Fetch `token_expiration` as a `String` from the row
                let token_expiration_str: String = match row.try_get("token_expiration") {
                    Ok(expiration) => expiration,
                    Err(_) => return Err("Failed to retrieve token expiration.".to_string()),
                };

                // Parse the token expiration string into NaiveDateTime
                let token_expires = match NaiveDateTime::parse_from_str(&token_expiration_str, "%Y-%m-%d %H:%M:%S") {
                    Ok(parsed_date) => parsed_date,
                    Err(_) => return Err("Failed to parse token expiration.".to_string()),
                };

                let now = Utc::now().naive_utc(); // Get the current time in naive UTC

                // Check if the token has expired
                if token_expires > now {
                    return Ok(Json(serde_json::json!({
                        "success": true,
                        "message": "Access granted."
                    })));
                } else {
                    // If the token is expired, clear it from the database
                    rocket_db_pools::sqlx::query(
                        "UPDATE admins SET token = NULL, token_expires = NULL WHERE token = ?",
                    )
                    .bind(token_value) // Use the cloned value here
                    .execute(&mut **db)
                    .await
                    .ok();

                    return Err("Token has expired.".to_string());
                }
            }
        }
        Err("No valid token found.".to_string())
    }


    #[post("/logout")]
    pub async fn logout(jar: &CookieJar<'_>, mut db: Connection<RoboDatabase>) {
        // Get the token from the cookie
        if let Some(token_cookie) = jar.get("token") {
            let token_value = token_cookie.value().to_string();

            // Remove the "token" cookie from the jar to log out the client
            jar.remove(Cookie::from("token"));

            // Set the token and token_expires fields to NULL for the user in the database
            let update_result = rocket_db_pools::sqlx::query(
                "UPDATE admins SET token = NULL, token_expiration = NULL WHERE token = ?",
            )
            .bind(&token_value)
            .execute(&mut **db)
            .await;

            // Optional: Check if the update succeeded
            if let Err(e) = update_result {
                eprintln!("Failed to clear token for user: {}", e);
            }
        }
    }

    #[allow(private_interfaces)]
    #[post("/create_admin", data = "<admin_form>")]
    pub async fn create_admin(
        admin_form: Form<CreateAdmin>,
        mut db: Connection<RoboDatabase>,
    ) -> Result<Json<ResponseData>, Status> {
        // Generate a random salt
        let salt: String = rand::thread_rng()
            .sample_iter(&Alphanumeric)
            .take(16) // You can adjust the length of the salt as needed
            .map(char::from)
            .collect();

        // Concatenate the salt with the password, then hash the combined string
        let salted_password = format!("{}{}", admin_form.password, salt);
        let hashed_password = hash_password(&salted_password);

        // SQL query to insert the new admin into the database
        let result = rocket_db_pools::sqlx::query(
            "INSERT INTO admins (username, salt, password, expiration) VALUES (?, ?, ?, ?)",
        )
        .bind(&admin_form.username)
        .bind(salt)
        .bind(hashed_password)
        .bind(&admin_form.expiration)
        .execute(&mut **db)
        .await;

        // Handle the result of the database operation
        match result {
            Ok(_) => {
                // Return a JSON response with a success flag
                Ok(Json(ResponseData {
                    success: true,
                    message: "Admin user created successfully.".to_string(),
                }))
            }
            Err(_) => {
                // Return a JSON response with an error message
                Err(Status::InternalServerError)
            }
        }
    }

    fn hash_password(password: &str) -> String {
        // Hashing logic using SHA-256
        let mut hasher = Sha256::new();
        hasher.update(password);
        let result = hasher.finalize();
        hex::encode(result) // Return the hex representation of the hash
    }

    #[derive(Serialize,Deserialize, FromForm)]
    struct CartItem {
        name: String,
        quantity: u32,
        price: f32,
    }

    #[allow(private_interfaces)]
    #[post("/addcart", data="<item>")]
    pub async fn add_cart(
        pot: &CookieJar<'_>,
        item: Json<CartItem>
    ) -> Json<usize> {
        
        // Retrieve the existing cart from the cookie, or initialize an empty cart
        let mut cart_items: Vec<CartItem> = if let Some(cookie) = pot.get("cart_items") {
            serde_json::from_str(cookie.value()).unwrap_or_default()
        } else {
            vec![]
        };

        //item to be added, maybe
        let mut new_item = item.into_inner();
        // Check if the item already exists in the cart
        if let Some(existing_item) = cart_items.iter_mut().find(|cart_item| cart_item.name == new_item.name) {
            // If the item exists, update its quantity
            existing_item.quantity += new_item.quantity;
        } else {
            // If the item does not exist, add it to the cart
            new_item.quantity = 1;
            cart_items.push(new_item);
        }

        // Convert the updated cart to a JSON string
        let cart_json = serde_json::to_string(&cart_items).unwrap();

        // Store the updated cart in the cookie
        pot.add(Cookie::new("cart_items", cart_json));

        Json(cart_items.len())
    }

    #[get("/getcart")]
    pub async fn get_cart(
        pot: &CookieJar<'_>
    ) -> String {
        // Retrieve the cart items from cookies, or return an empty array if not found
        let cart_items: Vec<CartItem> = if let Some(cookie) = pot.get("cart_items") {
            serde_json::from_str(cookie.value()).unwrap_or_default()
        } else {
            vec![]
        };

        // Convert cart items to JSON response
        serde_json::to_string(&cart_items).unwrap()
    }

    #[post("/removecart?<name>")]
    pub async fn remove_cart(
        pot: &CookieJar<'_>,
        name: String
    )-> Json<usize> {

        //let decoded_name = decode(&name)unwrap_or(name);

        // Retrieve the existing cart from the cookie
        let cart_items: Vec<CartItem> = if let Some(cookie) = pot.get("cart_items") {
            if let Ok(mut items) = serde_json::from_str::<Vec<CartItem>>(cookie.value()) {
                //if items.iter.any(|item| item.name == decoded_name){

                
                // Filter out the item to be removed
                items.retain(|item| item.name != name);
    
                // Update the cookie with the remaining items
                let updated_cart = serde_json::to_string(&items).unwrap();
                pot.add(Cookie::new("cart_items", updated_cart));
                items
                //}
            }else{
                // Retrieve the existing cart from the cookie, or initialize an empty cart
                let cart_items: Vec<CartItem> = if let Some(cookie) = pot.get("cart_items") {
                    serde_json::from_str(cookie.value()).unwrap_or_default()
                }else {
                    vec![]
                };
                cart_items
            }
        }else{
            vec![]
        };
        
        Json(cart_items.len())
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
        jar: &CookieJar<'_>,
    ) -> Result<&'static str, String> {
        match validate_user(jar.get("token").map(|x| x.value()).unwrap_or("")) {
            Some(_) => {}
            None => return Err("Not logged in".into()),
        };
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
    #[post("/modifyvariant", data = "<variant>")]
    pub(super) async fn mod_product_variant(
        variant: Json<ProductVariant>,
        mut db: Connection<RoboDatabase>,
        jar: &CookieJar<'_>,
    ) -> Result<&'static str, String> {
        match validate_user(jar.get("token").map(|x| x.value()).unwrap_or("")) {
            Some(_) => {}
            None => return Err("Not logged in".into()),
        };
        rocket_db_pools::sqlx::query(
            "UPDATE product_variants SET quantity = ?, tag_name = ? WHERE var_id = ?",
        )
        .bind(variant.quantity)
        .bind(
            variant
                .tags
                .iter()
                .map(|e| e.to_string())
                .reduce(|x, y| x + " " + &y),
        )
        .bind(variant.varid)
        .execute(&mut **db)
        .await
        .map_err(|e| e.to_string())?;

        Ok("ok")
    }

    #[allow(private_interfaces)]
    #[post("/addvariant", data = "<variant>")]
    pub(super) async fn add_product_variant(
        variant: Json<ProductVariant>,
        mut db: Connection<RoboDatabase>,
        jar: &CookieJar<'_>,
    ) -> Result<&'static str, String> {
        match validate_user(jar.get("token").map(|x| x.value()).unwrap_or("")) {
            Some(_) => {}
            None => return Err("Not logged in".into()),
        };
        rocket_db_pools::sqlx::query(
            "insert into product_variants (quantity, tag_name, product_id) values (?, ?, ?)",
        )
        .bind(variant.quantity)
        .bind(
            variant
                .tags
                .iter()
                .map(|e| e.to_string())
                .reduce(|x, y| x + " " + &y),
        )
        .bind(variant.product)
        .execute(&mut **db)
        .await
        .map_err(|e| e.to_string())?;

        Ok("ok")
    }

    #[allow(private_interfaces)]
    #[get("/get_websiteinfo")]
    pub(super) async fn get_websiteinfo(mut db: Connection<RoboDatabase>) -> Json<Vec<Description>> {
        let rows = rocket_db_pools::sqlx::query("select * from website_information")
            .fetch(&mut **db)
            .filter_map(|row| async move {
                let row = row.ok()?;
                Some(Description {
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
    pub(super) async fn update_websiteinfo(
        info: Form<WebsiteInfo>,
        mut db: Connection<RoboDatabase>,
    ) -> Result<Json<Value>, String> {
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
            Ok(_) => Ok(Json(serde_json::json!({
                "success": true,
                "message": "Website information updated successfully.",
            }))),
            Err(err) => Err(format!("Database error: {err}")),
        }
    }

    #[allow(private_interfaces)]
    #[get("/get_admins")]
    pub(super) async fn get_admins(
        mut db: Connection<RoboDatabase>,
    ) -> Result<Json<Vec<String>>, Status> {
        // SQL query to fetch all usernames from the admins table
        let usernames_query = rocket_db_pools::sqlx::query("SELECT username FROM admins")
            .fetch_all(&mut **db)
            .await;

        // Map rows to Vec<String> containing only usernames
        match usernames_query {
            Ok(rows) => {
                let usernames: Vec<String> = rows
                    .into_iter()
                    .map(|row| row.get("username"))
                    .collect();
                Ok(Json(usernames))
            }
            Err(_) => Err(Status::InternalServerError),
        }
    }


    #[allow(private_interfaces)]
    #[delete("/delete_admin/<username>")]
    pub(super) async fn delete_admin(
        username: &str,
        mut db: Connection<RoboDatabase>,
    ) -> Result<Json<ResponseData>, Status> {
        // SQL query to delete the admin by username
        let result = rocket_db_pools::sqlx::query("DELETE FROM admins WHERE username = ?")
            .bind(username)
            .execute(&mut **db)
            .await;

        // Handle the result of the database operation
        match result {
            Ok(_) => Ok(Json(ResponseData {
                success: true,
                message: "Admin user deleted successfully.".to_string(),
            })),
            Err(_) => Err(Status::InternalServerError),
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
                api::create_admin,
                api::login,
                api::logout,
                api::admin_menu,
                api::current_user,
                api::get_product_variants,
                api::mod_product_variant,
                api::add_product_variant,
                api::add_cart,
                api::get_cart,
                api::remove_cart,
                api::get_admins,
                api::delete_admin
            ],
        )
}
