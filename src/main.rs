use rocket::{
    fs::{FileServer, NamedFile},
    http::Method,
    Route,
};
use rocket_db_pools::Database;

#[macro_use]
extern crate rocket;

mod api {
    use crate::rocket::futures::TryFutureExt;
    use base64::engine::general_purpose::STANDARD;
    use base64::Engine;
    use chrono::{Duration, NaiveDate, NaiveDateTime, Utc};
    use rand::{distributions::Alphanumeric, Rng};
    use rocket::form::Form;
    use rocket::fs::TempFile;

    use rocket::http::Cookie;
    use rocket::http::CookieJar;
    use rocket::http::Status;
    use rocket::response::status::Custom;
    use rocket::tokio::io::AsyncReadExt;
    use rocket::{futures::StreamExt, serde::json::Json};
    use rocket_db_pools::sqlx::sqlite::SqliteRow;
    use rocket_db_pools::{
        sqlx::{Row, SqlitePool},
        Connection, Database,
    };
    use serde::de::{self, Visitor};
    use serde::{Deserialize, Deserializer, Serialize};
    use serde_json::Value;
    use sha2::{Digest, Sha256};
    use std::fmt;
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

    #[derive(Serialize, Deserialize, FromForm)]
    struct Product {
        id: Option<i32>,
        name: String,
        desc: String,
        price: f32,
        image: Option<std::string::String>, // Store the image as binary data
        quantity: f32,
    }

    impl TryFrom<SqliteRow> for Product {
        type Error = String;

        fn try_from(value: SqliteRow) -> Result<Self, Self::Error> {
            // Attempt to fetch the image blob from the database
            let image_blob: Option<Vec<u8>> = value.try_get("image").ok();

            // Convert the image blob to a Base64-encoded string
            let image_base64 = image_blob.map(|blob| STANDARD.encode(&blob));

            Ok(Self {
                id: value
                    .try_get("product_id")
                    .map_err(|e| format!("Could not get `name`: {e}"))?,
                name: value
                    .try_get("name")
                    .map_err(|e| format!("Could not get `name`: {e}"))?,
                desc: value
                    .try_get("desc")
                    .map_err(|e| format!("Could not get `desc`: {e}"))?,
                price: value
                    .try_get("price")
                    .map_err(|e| format!("Could not get `price`: {e}"))?,
                image: image_base64,
                quantity: value
                    .try_get("quantity")
                    .map_err(|e| format!("Could not get `quantity`: {e}"))?,
            })
        }
    }

    #[derive(Serialize, PartialEq)]
    enum VarTag {
        Size(String),
        Color(String),
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
                VarTag::Size(s) => format!("{}", s.to_lowercase().capitalize()),
                VarTag::Color(c) => format!("{}", c.to_lowercase().capitalize()),
                VarTag::Fitted(f) => {
                    if *f {
                        "Fitted".to_string()
                    } else {
                        "Snap".to_string()
                    }
                }
            }
        }
    }
    impl<'de> Deserialize<'de> for VarTag {
        fn deserialize<D>(deserializer: D) -> Result<VarTag, D::Error>
        where
            D: Deserializer<'de>,
        {
            struct VarTagVisitor;

            impl<'de> Visitor<'de> for VarTagVisitor {
                type Value = VarTag;

                fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                    write!(formatter, "a string representing a valid tag")
                }

                fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
                where
                    E: de::Error,
                {
                    if value.starts_with("size") {
                        Ok(VarTag::Size(value[4..].to_string()))
                    } else if value.starts_with("color") {
                        Ok(VarTag::Color(value[5..].to_string()))
                    } else if value == "fitted" {
                        Ok(VarTag::Fitted(true))
                    } else if value == "snap" {
                        Ok(VarTag::Fitted(false))
                    } else {
                        Err(de::Error::unknown_field(
                            value,
                            &["size", "color", "fitted"],
                        ))
                    }
                }
            }

            deserializer.deserialize_str(VarTagVisitor)
        }
    }
    #[derive(Serialize, Deserialize)]
    struct ProductVariant {
        quantity: Option<u32>,
        tag_name: Vec<VarTag>,
        product: u32,
        varid: Option<u32>,
        image: Option<std::string::String>,
    }

    impl TryFrom<SqliteRow> for ProductVariant {
        type Error = String;

        fn try_from(value: SqliteRow) -> Result<Self, Self::Error> {
            let image_blob: Option<Vec<u8>> = value.try_get("image").ok();

            // Convert the image blob to a Base64-encoded string
            let image_base64 = image_blob.map(|blob| STANDARD.encode(&blob));
            Ok(Self {
                quantity: value
                    .try_get::<Option<u32>, _>("quantity")
                    .map_err(|e| format!("Could not get `quantity` {e}"))?,
                tag_name: value
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
                image: image_base64,
            })
        }
    }

    trait Capitalize {
        fn capitalize(&self) -> String;
    }

    impl Capitalize for String {
        fn capitalize(&self) -> String {
            let mut c = self.chars();
            match c.next() {
                None => String::new(),
                Some(f) => f.to_uppercase().chain(c).collect(),
            }
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

        // If no valid token or username found, return None (to trigger the redirect)
        None
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

        let hashed_password = row.try_get::<String, _>("password").map_err(|e| {
            Custom(
                Status::InternalServerError,
                Json(ResponseData {
                    success: false,
                    message: format!("Database error: {}", e),
                }),
            )
        })?;
        let salt: String = row.try_get("salt").map_err(|e| {
            Custom(
                Status::InternalServerError,
                Json(ResponseData {
                    success: false,
                    message: format!("Database error: {}", e),
                }),
            )
        })?;
        let expiration_str: String = row.try_get("expiration").map_err(|e| {
            Custom(
                Status::InternalServerError,
                Json(ResponseData {
                    success: false,
                    message: format!("Database error: {}", e),
                }),
            )
        })?;

        // Parse the expiration date from the string in "YYYY-MM-DD" format
        let expiration_date =
            NaiveDate::parse_from_str(&expiration_str, "%Y-%m-%d").map_err(|e| {
                Custom(
                    Status::BadRequest,
                    Json(ResponseData {
                        success: false,
                        message: format!("Date parse error: {}", e),
                    }),
                )
            })?;

        // Get the current UTC date
        let now = Utc::now().naive_utc().date();

        // Check if the admin's expiration date has passed
        if now > expiration_date {
            // Remove the expired admin account from the database
            rocket_db_pools::sqlx::query("DELETE FROM admins WHERE username = ?")
                .bind(&login_form.username)
                .execute(&mut **db)
                .await
                .map_err(|e| {
                    Custom(
                        Status::InternalServerError,
                        Json(ResponseData {
                            success: false,
                            message: format!("Failed to remove expired admin: {}", e),
                        }),
                    )
                })?;

            return Err(Custom(
                Status::Unauthorized,
                Json(ResponseData {
                    success: false,
                    message: "Admin account has expired and has been removed.".into(),
                }),
            ));
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
            .map_err(|e| {
                Custom(
                    Status::InternalServerError,
                    Json(ResponseData {
                        success: false,
                        message: format!("Failed to update token: {}", e),
                    }),
                )
            })?;

            // Store the token in a cookie
            jar.add(Cookie::new("token", token));

            Ok(Json(ResponseData {
                success: true,
                message: "Login successful.".into(),
            }))
        } else {
            Err(Custom(
                Status::Unauthorized,
                Json(ResponseData {
                    success: false,
                    message: "Invalid username or password.".into(),
                }),
            ))
        }
    }

    /// Returns a username if the token is valid for the given permission
    async fn validate_user(
        token: &str,
        db: &mut Connection<RoboDatabase>,
        permission: &str,
    ) -> Result<String, &'static str> {
        // Validate the token
        let user = rocket_db_pools::sqlx::query("SELECT * FROM admins WHERE token = ?")
            .bind(token)
            .fetch_one(&mut ***db)
            .await;

        if let Ok(row) = user {
            // Fetch `token_expiration` as a `String` from the row
            let token_expiration_str: String = match row.try_get("token_expiration") {
                Ok(expiration) => expiration,
                Err(_) => return Err("Failed to retrieve token expiration."),
            };

            // Parse the token expiration string into NaiveDateTime
            let token_expires =
                match NaiveDateTime::parse_from_str(&token_expiration_str, "%Y-%m-%d %H:%M:%S") {
                    Ok(parsed_date) => parsed_date,
                    Err(_) => return Err("Failed to parse token expiration."),
                };

            let now = Utc::now().naive_utc(); // Get the current time in naive UTC

            // Check if the token has expired
            if token_expires > now {
                let username = row
                    .try_get::<String, _>("username")
                    .map_err(|_| "Could not find username in admins")?;
                let perms =
                    rocket_db_pools::sqlx::query("SELECT * FROM permissions WHERE username = ?")
                        .bind(&username)
                        .fetch(&mut ***db);
                // .map_err(|_| "Could not find permissions in table")?;
                let maybe_perm = perms
                    .filter(|row| {
                        let perm = row
                            .as_ref()
                            .map(|r| r.get::<String, _>("permission"))
                            .unwrap_or(String::new());
                        async move { &perm == permission }
                    })
                    .collect::<Vec<_>>()
                    .await;
                if !maybe_perm.is_empty() {
                    Ok(username)
                } else {
                    Err("User did not have correct permissions")
                }
            } else {
                // If the token is expired, clear it from the database
                rocket_db_pools::sqlx::query(
                    "UPDATE admins SET token = NULL, token_expires = NULL WHERE token = ?",
                )
                .bind(token) // Use the cloned value here
                .execute(&mut ***db)
                .await
                .map_err(|_| "Could not remove token from database")?;

                Err("Token has expired.")
            }
        } else {
            Err("Token does not exist.")
        }
    }

    #[get("/admin_menu")]
    pub async fn admin_menu(
        jar: &CookieJar<'_>,
        mut db: Connection<RoboDatabase>,
    ) -> Result<Json<Value>, String> {
        let token = jar.get("token").map(|c| c.value().to_string());

        if let Some(token_value) = token {
            match validate_user(&token_value, &mut db, "admin").await {
                Ok(_) => {
                    return Ok(Json(serde_json::json!({
                        "success": true,
                        "message": "Access granted."
                    })));
                }
                Err(e) => return Err(e.to_string()),
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
        jar: &CookieJar<'_>,
        mut db: Connection<RoboDatabase>,
    ) -> Result<Json<ResponseData>, Status> {
        let token = jar.get("token").map(|c| c.value().to_string());

        if let Some(token_value) = token {
            match validate_user(&token_value, &mut db, "admincreate").await {
                Ok(_) => {}
                Err(_) => return Err(Status::Unauthorized),
            }
        }
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
            }
            Err(_) => {
                // Return a JSON response with an error message
                return Err(Status::InternalServerError);
            }
        }
        let result = rocket_db_pools::sqlx::query(
            "INSERT INTO permissions (username, permission) VALUES (?, 'admin')",
        )
        .bind(&admin_form.username)
        .execute(&mut **db)
        .await;
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

    #[derive(Serialize, Deserialize, FromForm)]
    struct CartItem {
        product: i32,
        name: String,    // Common name (product or variant name)
        quantity: u32,   // Quantity of the item
        price: f32,      // Price of the item
        variant: String, // Variant-specific data (if applicable)
    }

    #[allow(private_interfaces)]
    #[post("/addcart", data = "<item>")]
    pub async fn add_cart(pot: &CookieJar<'_>, item: Json<CartItem>) -> Json<usize> {
        // Retrieve the existing cart from the cookie, or initialize an empty cart
        let mut cart_items: Vec<CartItem> = if let Some(cookie) = pot.get("cart_items") {
            serde_json::from_str(cookie.value()).unwrap_or_default()
        } else {
            vec![]
        };

        // Extract the item to be added
        let mut new_item = item.into_inner();

        // Check if the item already exists in the cart (considering both name and variant)
        if let Some(existing_item) = cart_items.iter_mut().find(|cart_item| {
            // Compare name and variant (check if both are equal, including the variant details)
            cart_item.name == new_item.name &&
            // Handle the variant comparison explicitly
            cart_item.variant == new_item.variant
        }) {
            // If the item exists, update its quantity
            existing_item.quantity += new_item.quantity;
        } else {
            // If the item does not exist, add it to the cart
            new_item.quantity = 1; // Ensure the quantity starts at 1
            cart_items.push(new_item);
        }

        // Convert the updated cart to a JSON string
        let cart_json = serde_json::to_string(&cart_items).unwrap();

        // Store the updated cart in the cookie
        pot.add(Cookie::new("cart_items", cart_json));

        // Calculate the total number of items (sum of quantities)
        let total_items = cart_items.iter().map(|item| item.quantity).sum::<u32>();

        // Return the total number of items in the cart
        Json(total_items as usize)
    }

    #[get("/getcart")]
    pub async fn get_cart(pot: &CookieJar<'_>) -> String {
        // Retrieve the cart items from cookies, or return an empty array if not found
        let cart_items: Vec<CartItem> = if let Some(cookie) = pot.get("cart_items") {
            serde_json::from_str(cookie.value()).unwrap_or_default()
        } else {
            vec![]
        };

        // Convert cart items to JSON response
        serde_json::to_string(&cart_items).unwrap()
    }

    #[get("/get_cart_count")]
    pub async fn get_cart_count(pot: &CookieJar<'_>) -> Json<i32> {
        // Retrieve the cart items from cookies, or return 0 if no cart exists
        let cart_items: Vec<CartItem> = if let Some(cookie) = pot.get("cart_items") {
            serde_json::from_str(cookie.value()).unwrap_or_default()
        } else {
            vec![]
        };

        // Calculate the total quantity of items in the cart
        let total_quantity: i32 = cart_items.iter().map(|item| item.quantity as i32).sum();

        // Return the total quantity as an i32
        Json(total_quantity)
    }

    #[post("/removecart?<name>&<variant>")]
    pub async fn remove_cart(pot: &CookieJar<'_>, name: String, variant: String) -> Json<usize> {
        // Retrieve the existing cart from the cookie
        let cart_items: Vec<CartItem> = if let Some(cookie) = pot.get("cart_items") {
            if let Ok(mut items) = serde_json::from_str::<Vec<CartItem>>(cookie.value()) {
                // Filter out the item to be removed by matching both name and variant
                items.retain(|item| item.name != name || item.variant != variant);

                // Update the cookie with the remaining items
                let updated_cart = serde_json::to_string(&items).unwrap();
                pot.add(Cookie::new("cart_items", updated_cart));
                items
            } else {
                // Retrieve the existing cart from the cookie, or initialize an empty cart
                let cart_items: Vec<CartItem> = if let Some(cookie) = pot.get("cart_items") {
                    serde_json::from_str(cookie.value()).unwrap_or_default()
                } else {
                    vec![]
                };
                cart_items
            }
        } else {
            vec![]
        };

        Json(cart_items.len())
    }

    #[post("/clearcart")]
    pub async fn clear_cart(pot: &CookieJar<'_>) -> Json<Result<usize, String>> {
        // Remove the "cart_items" cookie by setting it to an empty value
        pot.remove(Cookie::new("cart_items", ""));

        // Return a success response
        Json(Ok(1)) // Return 1 for success
    }

    #[allow(private_interfaces)]
    #[get("/get_items")]
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

    #[allow(private_interfaces)]
    #[post("/add_product", data = "<new_product>")]
    pub(super) async fn add_product(
        new_product: Json<Product>,
        mut db: Connection<RoboDatabase>,
        jar: &CookieJar<'_>,
    ) -> Result<Json<i32>, String> {
        match validate_user(
            jar.get("token").map(|x| x.value()).unwrap_or(""),
            &mut db,
            "addproduct",
        )
        .await
        {
            Ok(_) => {}
            Err(e) => return Err(format!("Not logged in: {e}")),
        };

        let mut item_to_add = new_product.into_inner();

        // Round the price to exactly 2 decimal places
        let formatted_price = (item_to_add.price * 100.0).round() / 100.0;
        item_to_add.price = formatted_price;

        // Insert the new product into the database without specifying the ID (let the DB auto-generate it)
        let result = rocket_db_pools::sqlx::query(
            "insert into products (name, desc, price, quantity) values ($1, $2, $3, $4) returning product_id",
        )
        .bind(&item_to_add.name)
        .bind(&item_to_add.desc)
        .bind(&item_to_add.price)
        .bind(&item_to_add.quantity)
        .fetch_one(&mut **db)
        .await;

        match result {
            Ok(row) => {
                // Extract the generated ID from the result
                let product_id: i32 = row
                    .try_get("product_id")
                    .map_err(|e| format!("Error extracting ID: {}", e))?;

                // Return the ID as JSON
                Ok(Json(product_id))
            }
            Err(e) => Err(format!("Database error: {}", e)),
        }
    }

    #[allow(private_interfaces)]
    #[post("/update_product", data = "<updated_product>")]
    pub(super) async fn update_product(
        updated_product: Json<Product>, // Handle the updated form data
        mut db: Connection<RoboDatabase>,
        jar: &CookieJar<'_>,
    ) -> Result<Json<i32>, String> {
        // Return the product_id as a String
        // Validate the user's session
        match validate_user(
            jar.get("token").map(|x| x.value()).unwrap_or(""),
            &mut db,
            "updateproduct",
        )
        .await
        {
            Ok(_) => {}
            Err(e) => return Err(format!("Not logged in: {e}")),
        };

        let product = updated_product.into_inner();

        // Update product in the database
        let update_result = rocket_db_pools::sqlx::query(
            "UPDATE products SET 'desc' = $1, price = $2, quantity = $3 WHERE name = $4 RETURNING product_id"
        )
        .bind(&product.desc)
        .bind(&product.price)
        .bind(&product.quantity)
        .bind(&product.name)
        .fetch_one(&mut **db)
        .await;

        match update_result {
            Ok(row) => {
                // Retrieve the product_id from the returned row
                let product_id: i32 = row.get("product_id");
                Ok(Json(product_id)) // Return the product_id as a string
            }
            Err(e) => Err(format!("Error updating product: {e}")),
        }
    }

    #[allow(private_interfaces)]
    #[delete("/remove_product/<product_name>")]
    pub(super) async fn remove_product(
        product_name: &str, // Parameter type still as String
        mut db: Connection<RoboDatabase>,
        jar: &CookieJar<'_>,
    ) -> Result<Json<String>, String> {
        match validate_user(
            jar.get("token").map(|x| x.value()).unwrap_or(""),
            &mut db,
            "removeproduct",
        )
        .await
        {
            Ok(_) => {}
            Err(e) => return Err(format!("Not logged in: {e}")),
        };
        // Retrieve the product_id of the product to delete
        let product_result =
            rocket_db_pools::sqlx::query("SELECT product_id FROM products WHERE name = ?")
                .bind(&product_name) // Bind the product name to the query
                .fetch_one(&mut **db) // Fetch the row
                .await;

        // Check if the product exists and extract the product_id
        let product_id = match product_result {
            Ok(row) => row.get::<i32, _>("product_id"), // Extract product_id (assuming it's of type i32)
            Err(e) => {
                return Err(format!("Failed to find product: {}", e)); // Return error if the product is not found
            }
        };

        // First, remove variants associated with the product
        let variant_result =
            rocket_db_pools::sqlx::query("DELETE FROM product_variants WHERE product_id = ?")
                .bind(product_id) // Bind the product_id to the query
                .execute(&mut **db) // Execute the delete query within the transaction
                .await;

        match variant_result {
            Ok(query_result) => {
                if query_result.rows_affected() == 0 {
                    // No variants were removed, which might be fine, so continue
                    println!("No variants found for product '{}'", product_name);
                }
            }
            Err(e) => {
                return Err(format!("Failed to remove variants: {}", e));
            }
        }

        // Now, remove the product
        let product_result = rocket_db_pools::sqlx::query("DELETE FROM products WHERE name = ?")
            .bind(&product_name) // Bind the product name to the query
            .execute(&mut **db) // Execute the delete query within the transaction
            .await;

        match product_result {
            Ok(query_result) => {
                if query_result.rows_affected() > 0 {
                    // Commit the transaction if both delete operations were successful
                    Ok(Json(
                        "Product and associated variants removed successfully.".to_string(),
                    ))
                } else {
                    // No product found with the given name
                    Err("Product not found.".to_string())
                }
            }
            Err(e) => {
                // Return an error if the product deletion fails
                Err(format!("Failed to remove product: {}", e))
            }
        }
    }

    #[allow(private_interfaces)]
    #[get("/get_product_variants?<name>")]
    pub(super) async fn get_product_variants(
        name: String,
        mut db: Connection<RoboDatabase>,
    ) -> Result<Json<Vec<serde_json::Value>>, String> {
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
        let mut formatted_prod_vars = vec![];
        for prodvar in prod_vars {
            match prodvar {
                Ok(variant) => {
                    formatted_prod_vars.push(serde_json::json!({
                        "quantity": variant.quantity,
                        "tag_name": variant
                            .tag_name
                            .iter()
                            .map(|tag| tag.to_string()) // Format each tag name
                            .collect::<Vec<String>>()
                            .join(" "), // Join all tags into a single string
                        "product": variant.product,
                        "varid": variant.varid,
                        "image": variant.image,
                    }));
                }
                Err(e) => return Err(e),
            }
        }
        // Step 4: Return the transformed variants as JSON
        Ok(Json(formatted_prod_vars))
    }

    #[allow(private_interfaces)]
    #[get("/get_variant_id?<product_id>&<tag_name>")]
    pub(super) async fn get_variant_id(
        product_id: u32,
        tag_name: String,
        mut db: Connection<RoboDatabase>,
    ) -> Result<Json<u32>, String> {
        // Split the tag_name into two strings based on whitespace
        let tags: Vec<&str> = tag_name.split_whitespace().collect();

        // Ensure there are at least two tags to work with, if not return an error
        if tags.len() < 2 {
            return Err("tag_name must contain at least two words".to_string());
        }

        // Create the formatted tags with wildcards for partial matching
        let tag1 = format!("%{}%", tags[0]); // First part of the tag
        let tag2 = format!("%{}%", tags[1]); // Second part of the tag

        // Query to find the var_id for the given product_id and both tag names
        let var_id: u32 = rocket_db_pools::sqlx::query(
            "SELECT var_id FROM product_variants WHERE product_id = $1 AND tag_name LIKE $2 AND tag_name LIKE $3",
        )
        .bind(product_id)
        .bind(tag1) // Use the formatted first part of the tag
        .bind(tag2) // Use the formatted second part of the tag
        .fetch_one(&mut **db)
        .await
        .map_err(|e| format!("Could not find variant for product_id {product_id} and tag_name {tag_name}: {e}"))?
        .try_get("var_id")
        .map_err(|e| format!("Could not get var_id from the database: {e}"))?;

        // Return the var_id as a JSON response
        Ok(Json(var_id))
    }

    #[allow(private_interfaces)]
    #[post("/modify_variant", data = "<variant>")]
    pub(super) async fn modify_variant(
        variant: Json<ProductVariant>,
        mut db: Connection<RoboDatabase>,
        jar: &CookieJar<'_>,
    ) -> Result<&'static str, String> {
        match validate_user(
            jar.get("token").map(|x| x.value()).unwrap_or(""),
            &mut db,
            "updatevariant",
        )
        .await
        {
            Ok(_) => {}
            Err(e) => return Err(format!("Not logged in: {e}")),
        };
        rocket_db_pools::sqlx::query(
            "UPDATE product_variants SET quantity = ?, tag_name = ? WHERE var_id = ?",
        )
        .bind(variant.quantity)
        .bind(
            variant
                .tag_name
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
    #[post("/add_variant", data = "<variant>")]
    pub(super) async fn add_product_variant(
        variant: Json<ProductVariant>,
        mut db: Connection<RoboDatabase>,
        jar: &CookieJar<'_>,
    ) -> Result<Json<i32>, String> {
        // Validate the user's token (authentication)
        match validate_user(
            jar.get("token").map(|x| x.value()).unwrap_or(""),
            &mut db,
            "addvariant",
        )
        .await
        {
            Ok(_) => {}
            Err(e) => return Err(format!("Not logged in: {e}")),
        };

        // Map tags to their categories
        let tag_mapping = vec![
            ("small", VarTag::Size("small".to_string())),
            ("medium", VarTag::Size("medium".to_string())),
            ("large", VarTag::Size("large".to_string())),
            ("white", VarTag::Color("white".to_string())),
            ("red", VarTag::Color("red".to_string())),
            ("blue", VarTag::Color("blue".to_string())),
            ("fitted", VarTag::Fitted(true)),
            ("snap", VarTag::Fitted(false)),
        ];

        // Validate and normalize the tags
        let normalized_tags: Vec<String> = variant
            .tag_name
            .iter()
            .map(|tag| {
                // Map the tag to its corresponding VarTag variant
                tag_mapping
                    .iter()
                    .find(|(_, category)| *category == *tag)
                    .map(|(tag_value, _)| tag_value.to_string()) // Convert to string
            })
            .filter_map(|t| t) // Filter out invalid tags that couldn't be mapped
            .collect();

        // Ensure all tags are valid (i.e., they map to recognized categories)
        if normalized_tags.is_empty() {
            return Err("Invalid tag name".into());
        }

        // Combine the valid tags into a single string (e.g., "size small color white")
        let combined_tags = normalized_tags.join(" ");

        // Insert into the database and return the generated ID (var_id)
        let result = rocket_db_pools::sqlx::query(
            "insert into product_variants (quantity, tag_name, product_id) values (?, ?, ?) RETURNING var_id",
        )
        .bind(variant.quantity)
        .bind(combined_tags) // Use the combined, normalized tags string
        .bind(variant.product)
        .fetch_one(&mut **db)
        .await;

        match result {
            Ok(row) => {
                // Extract the generated ID (var_id) from the result
                let var_id: i32 = row
                    .try_get("var_id")
                    .map_err(|e| format!("Error extracting ID: {}", e))?;

                // Return the ID as JSON
                Ok(Json(var_id))
            }
            Err(e) => Err(format!("Database error: {}", e)),
        }
    }

    #[allow(private_interfaces)]
    #[get("/get_websiteinfo")]
    pub(super) async fn get_websiteinfo(
        mut db: Connection<RoboDatabase>,
    ) -> Json<Vec<Description>> {
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
        jar: &CookieJar<'_>,
    ) -> Result<Json<Value>, String> {
        match validate_user(
            jar.get("token").map(|x| x.value()).unwrap_or(""),
            &mut db,
            "websiteinfo",
        )
        .await
        {
            Ok(_) => {}
            Err(e) => return Err(format!("Not logged in: {e}")),
        };
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

    #[post("/makeimage", data = "<image>")]
    pub(super) async fn make_image(
        mut image: Form<TempFile<'_>>,
        mut db: Connection<RoboDatabase>,
        jar: &CookieJar<'_>,
    ) -> Result<Json<String>, String> {
        match validate_user(
            jar.get("token").map(|x| x.value()).unwrap_or(""),
            &mut db,
            "image",
        )
        .await
        {
            Ok(_) => {}
            Err(e) => return Err(format!("Not logged in: {e}")),
        };

        let tfile = image.open();
        let mut contents = String::new();
        tfile
            .await
            .map_err(|e| format!("File didn't upload {e}"))?
            .read_to_string(&mut contents)
            .await
            .map_err(|e| format!("Couldnt read file {e}"))?;
        let contents = contents;

        let name = hash_password(&contents);
        let ctype = image
            .content_type()
            .ok_or("No file type detected")?
            .extension()
            .ok_or("File type not recognized")?
            .to_string();
        image
            .persist_to(format!("images/{name}.{ctype}",))
            .await
            .map_err(|e| format!("Couldn't save file {e}"))?;
        Ok(Json(format!("{name}.{ctype}")))
    }

    #[allow(private_interfaces)]
    #[get("/get_admins")]
    pub(super) async fn get_admins(
        mut db: Connection<RoboDatabase>,
        jar: &CookieJar<'_>,
    ) -> Result<Json<Vec<String>>, Status> {
        match validate_user(
            jar.get("token").map(|x| x.value()).unwrap_or(""),
            &mut db,
            "adminlist",
        )
        .await
        {
            Ok(_) => {}
            Err(_) => return Err(Status::Unauthorized),
        };
        // SQL query to fetch all usernames from the admins table
        let usernames_query = rocket_db_pools::sqlx::query("SELECT username FROM admins")
            .fetch_all(&mut **db)
            .await;

        // Map rows to Vec<String> containing only usernames
        match usernames_query {
            Ok(rows) => {
                let usernames: Vec<String> =
                    rows.into_iter().map(|row| row.get("username")).collect();
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
        jar: &CookieJar<'_>,
    ) -> Result<Json<ResponseData>, Status> {
        match validate_user(
            jar.get("token").map(|x| x.value()).unwrap_or(""),
            &mut db,
            "admindelete",
        )
        .await
        {
            Ok(_) => {}
            Err(_) => return Err(Status::Unauthorized),
        };
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

    // Structs for customers, orders, and products
    #[derive(Serialize, Deserialize)]
    struct Customer {
        cust_id: Option<i32>,
        name: String,
        address: Address,
        email: String,
        phone_number: Option<String>,
    }

    #[derive(Serialize)]
    struct Order {
        order_id: i32,
        products: Vec<OrderedItem>,
    }

    #[derive(Serialize, Deserialize)]
    struct OrderedItem {
        product_id: i32,
        variant: Option<i32>,
        quantity: i32,
    }

    #[derive(Serialize, Deserialize)]
    struct Address {
        address_line_1: String,
        admin_area_2: String, // City
        admin_area_1: String, // State
        postal_code: String,
        country_code: String,
    }

    // Fetch all customers
    #[allow(private_interfaces)]
    #[get("/getallcustomers")]
    pub(super) async fn get_all_customers(
        mut db: Connection<RoboDatabase>,
        jar: &CookieJar<'_>,
    ) -> Result<Json<Vec<Customer>>, Status> {
        match validate_user(
            jar.get("token").map(|x| x.value()).unwrap_or(""),
            &mut db,
            "customer",
        )
        .await
        {
            Ok(_) => {}
            Err(_) => return Err(Status::Unauthorized),
        };
        let query = "SELECT cust_id, name, address, email, phone_number FROM customers";

        let rows = rocket_db_pools::sqlx::query(query)
            .fetch_all(&mut **db)
            .await
            .map_err(|_| Status::InternalServerError)?;

        let customers: Vec<Customer> = rows
            .into_iter()
            .filter_map(|row| {
                let cust_id: Option<i32> = row.get("cust_id");
                let name: String = row.get("name");
                let address_str: String = row.get("address");
                let email: String = row.get("email");
                let phone_number: Option<String> = row.get("phone_number");

                // Parse the formatted address string
                let address_parts: Vec<&str> = address_str.split(',').collect();
                if address_parts.len() == 5 {
                    let address = Address {
                        address_line_1: address_parts[0].trim().to_string(),
                        admin_area_2: address_parts[1].trim().to_string(),
                        admin_area_1: address_parts[2].trim().to_string(),
                        postal_code: address_parts[3].trim().to_string(),
                        country_code: address_parts[4].trim().to_string(),
                    };

                    Some(Customer {
                        cust_id,
                        name,
                        address,
                        email,
                        phone_number,
                    })
                } else {
                    None
                }
            })
            .collect();

        Ok(Json(customers))
    }

    // Fetch orders for a specific customer
    #[allow(private_interfaces)]
    #[get("/getcustomerorders/<cust_id>")]
    pub(super) async fn get_customer_orders(
        cust_id: i32,
        mut db: Connection<RoboDatabase>,
        jar: &CookieJar<'_>,
    ) -> Result<Json<Vec<Order>>, Status> {
        match validate_user(
            jar.get("token").map(|x| x.value()).unwrap_or(""),
            &mut db,
            "customer",
        )
        .await
        {
            Ok(_) => {}
            Err(_) => return Err(Status::Unauthorized),
        };
        let query = "SELECT order_id FROM orders WHERE cust_id = ?";
        let rows = rocket_db_pools::sqlx::query(query)
            .bind(cust_id)
            .fetch_all(&mut **db)
            .await
            .map_err(|_| Status::InternalServerError)?;

        let mut orders = Vec::new();
        for row in rows {
            let order_id: i32 = row.get("order_id");

            // Fetch ordered items for the current order
            let products_query =
                "SELECT product_id, var_id, quantity FROM ordered_products WHERE order_id = ?";
            let products_rows = rocket_db_pools::sqlx::query(products_query)
                .bind(order_id)
                .fetch_all(&mut **db)
                .await
                .map_err(|_| Status::InternalServerError)?;

            let products: Vec<OrderedItem> = products_rows
                .into_iter()
                .filter_map(|row| {
                    let product_id: i32 = row.get("product_id");
                    let variant: Option<i32> = row.get("var_id");
                    let quantity: i32 = row.get("quantity");

                    Some(OrderedItem {
                        product_id,
                        variant,
                        quantity,
                    })
                })
                .collect();

            orders.push(Order { order_id, products });
        }

        Ok(Json(orders))
    }

    #[derive(Deserialize)]
    struct OrderRequest {
        customer: Customer,
        items: Vec<OrderedItem>,
    }

    #[allow(private_interfaces)]
    #[post("/create_order", data = "<order_data>")]
    pub(super) async fn create_order(
        order_data: Json<OrderRequest>,
        mut db: Connection<RoboDatabase>,
    ) -> Result<Json<i32>, String> {
        let customer = &order_data.customer;
        let formatted_address = format!(
            "{}, {}, {}, {}, {}",
            customer.address.address_line_1,
            customer.address.admin_area_2,
            customer.address.admin_area_1,
            customer.address.postal_code,
            customer.address.country_code
        );
        let items = &order_data.items;

        // Step 1: Insert customer information and get the generated `cust_id`
        let cust_id: i32 = rocket_db_pools::sqlx::query(
            r#"
            INSERT INTO customers (name, address, email, phone_number)
            VALUES ($1, $2, $3, $4)
            RETURNING cust_id
            "#,
        )
        .bind(&customer.name)
        .bind(&formatted_address)
        .bind(&customer.email)
        .bind(&customer.phone_number)
        .fetch_one(&mut **db)
        .await
        .map_err(|e| format!("Failed to insert customer: {}", e))?
        .try_get("cust_id")
        .map_err(|e| format!("Failed to get cust_id: {}", e))?;

        // Step 2: Insert a new order and get the generated `order_id`
        let order_id: i32 = rocket_db_pools::sqlx::query(
            r#"
            INSERT INTO orders (cust_id)
            VALUES ($1)
            RETURNING order_id
            "#,
        )
        .bind(cust_id)
        .fetch_one(&mut **db)
        .await
        .map_err(|e| format!("Failed to insert order: {}", e))?
        .try_get("order_id")
        .map_err(|e| format!("Failed to get order_id: {}", e))?;

        // Step 3: Insert ordered products
        for item in items {
            rocket_db_pools::sqlx::query(
                r#"
                INSERT INTO ordered_products (product_id, var_id, order_id, quantity)
                VALUES ($1, $2, $3, $4)
                "#,
            )
            .bind(item.product_id)
            .bind(item.variant) // This can be NULL
            .bind(order_id)
            .bind(item.quantity)
            .execute(&mut **db)
            .await
            .map_err(|e| format!("Failed to insert ordered product: {}", e))?;
        }

        // Step 4: Return the order_id
        Ok(Json(order_id)) // Return the generated order_id
    }

    #[allow(private_interfaces)]
    #[get("/get_product_details?<name>")]
    pub(super) async fn get_product_details(
        mut db: Connection<RoboDatabase>,
        name: String,
    ) -> Result<Json<Product>, String> {
        // Query the database for the product by ID
        let row = rocket_db_pools::sqlx::query("SELECT * FROM products WHERE name = $1")
            .bind(name)
            .fetch_one(&mut **db)
            .await
            .map_err(|e| format!("Error fetching product: {e}"))?;

        // Manually map the row to a Product struct
        let product = Product {
            id: row.try_get("product_id").ok(),
            name: row.try_get("name").unwrap_or_default(),
            desc: row.try_get("desc").unwrap_or_default(),
            price: row.try_get("price").unwrap_or_default(),
            image: row.try_get("image").ok(),
            quantity: row.try_get("quantity").unwrap_or_default(),
        };

        // If the query succeeds, return the product in JSON format
        Ok(Json(product))
    }

    #[allow(private_interfaces)]
    #[get("/get_variant_details?<name>")]
    pub(super) async fn get_variant_details(
        mut db: Connection<RoboDatabase>,
        name: String,
    ) -> Result<Json<ProductVariant>, String> {
        // Query the database for the product variant by name
        let row = rocket_db_pools::sqlx::query("SELECT * FROM product_variants WHERE var_id = $1")
            .bind(name)
            .fetch_one(&mut **db)
            .await
            .map_err(|e| format!("Error fetching product variant: {e}"))?;

        // Deserialize the tag_name field if it exists
        let tag: Vec<VarTag> = match row.try_get::<Option<String>, _>("tag_name") {
            Ok(Some(value)) => serde_json::from_str(&value).unwrap_or_default(),
            Ok(None) => Vec::new(),
            Err(_) => return Err("Failed to parse tag_name".to_string()),
        };

        // Manually map the row to a ProductVariant struct
        let product = ProductVariant {
            tag_name: tag,
            product: row.try_get("product_id").unwrap_or_default(),
            varid: row.try_get("var_id").unwrap_or_default(),
            image: row.try_get("image").ok(),
            quantity: row.try_get("quantity").unwrap_or_default(),
        };

        // If the query succeeds, return the product variant in JSON format
        Ok(Json(product))
    }
}

// Route to set homepage.html on run
#[get("/")]
async fn homepage() -> Option<NamedFile> {
    NamedFile::open("./pages/homepage.html").await.ok()
}

#[launch]
async fn rocket() -> _ {
    let mut rootroutes = [
        Route::new(Method::Get, "/<path..>", FileServer::from("./pages")),
        Route::new(
            Method::Get,
            "/<path..>",
            FileServer::from("./product_images"),
        ),
    ];
    rootroutes[0].rank = -2;
    rootroutes[1].rank = -1;
    rocket::build()
        .attach(api::RoboDatabase::init())
        .mount("/", routes![homepage])
        .mount("/", rootroutes)
        .mount(
            "/api",
            routes![
                api::get_items,
                api::get_websiteinfo,
                api::update_websiteinfo,
                api::create_admin,
                api::login,
                api::logout,
                api::admin_menu,
                api::current_user,
                api::get_product_variants,
                api::get_variant_id,
                api::modify_variant,
                api::add_product_variant,
                api::make_image,
                api::add_cart,
                api::get_cart,
                api::get_cart_count,
                api::remove_cart,
                api::get_admins,
                api::delete_admin,
                api::add_product,
                api::update_product,
                api::remove_product,
                api::create_order,
                api::clear_cart,
                api::get_product_details,
                api::get_variant_details,
                api::get_customer_orders,
                api::get_all_customers,
            ],
        )
}
