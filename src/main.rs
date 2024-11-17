use rocket::{
    fs::{FileServer, NamedFile},
    http::Method,
    Route,
};
use rocket_db_pools::Database;

#[macro_use]
extern crate rocket;

mod api {
    use chrono::{Duration, NaiveDate, NaiveDateTime, Utc};
    use rand::{distributions::Alphanumeric, Rng};
    use rocket::form::Form;
    use rocket::fs::TempFile;
    use crate::rocket::futures::TryFutureExt;
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
    use serde::{Deserialize, Serialize};
    use serde_json::Value;
    use sha2::{Digest, Sha256};
    use std::str::FromStr;
    use uuid::Uuid;
    use base64::Engine;
    use base64::engine::general_purpose::STANDARD;

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
        id: i32,
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

    #[derive(Serialize, Deserialize)]
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
    #[derive(Serialize, Deserialize)]
    struct ProductVariant {
        quantity: Option<u32>,
        tag_name: Vec<VarTag>,
        product: u32,
        varid: u32,
        image: Option<std::string::String>
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
                let token_expires =
                    match NaiveDateTime::parse_from_str(&token_expiration_str, "%Y-%m-%d %H:%M:%S")
                    {
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

    #[derive(Serialize, Deserialize, FromForm)]
    struct CartItem {
        product: i32,
        name: String,           // Common name (product or variant name)
        quantity: u32,          // Quantity of the item
        price: f32,             // Price of the item
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

    #[post("/removecart?<name>")]
    pub async fn remove_cart(pot: &CookieJar<'_>, name: String) -> Json<usize> {
        //let decoded_name = decode(&name)unwrap_or(name);

        // Retrieve the existing cart from the cookie
        let cart_items: Vec<CartItem> = if let Some(cookie) = pot.get("cart_items") {
            if let Ok(mut items) = serde_json::from_str::<Vec<CartItem>>(cookie.value()) {

                // Filter out the item to be removed
                items.retain(|item| item.name != name);

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
        Json(Ok(1))  // Return 1 for success
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
    #[post("/addproduct", data = "<new_product>")]
    pub(super) async fn add_product(
        new_product: Json<Product>,
        mut db: Connection<RoboDatabase>,
        jar: &CookieJar<'_>,
    ) -> Result<&'static str, String> {
        match validate_user(jar.get("token").map(|x| x.value()).unwrap_or("")) {
            Some(_) => {}
            None => return Err("Not logged in".into()),
        };

        let mut itemtoadd = new_product.into_inner();

        // Round the price to exactly 2 decimal places
        let formatted_price = (itemtoadd.price * 100.0).round() / 100.0;
        itemtoadd.price = formatted_price;

        rocket_db_pools::sqlx::query(
            "insert into products (name, desc, price, quantity) values ($1, $2 ,$3, $4)",
        )
        .bind(&itemtoadd.name)
        .bind(&itemtoadd.desc)
        .bind(&itemtoadd.price)
        .bind(&itemtoadd.quantity)
        .execute(&mut **db)
        .await
        .map_err(|e| format!("Database error {e}"))?;
        Ok("Added")
    }

    #[allow(private_interfaces)]
    #[post("/updateproduct", data = "<updated_product>")]
    pub(super) async fn update_product(
        updated_product: Json<Product>,  // Handle the updated form data
        mut db: Connection<RoboDatabase>,
        jar: &CookieJar<'_>,
    ) -> Result<&'static str, String> {
        match validate_user(jar.get("token").map(|x| x.value()).unwrap_or("")) {
            Some(_) => {}
            None => return Err("Not logged in".into()),
        };

        let product = updated_product.into_inner();

        // Update product in the database
        let result = rocket_db_pools::sqlx::query(
            "UPDATE products SET desc = $1, price = $2, quantity = $3 WHERE name = $4"
        )
        .bind(&product.desc)
        .bind(&product.price)
        .bind(&product.quantity)
        .bind(&product.name)
        .execute(&mut **db)
        .await;

        match result {
            Ok(_) => Ok("Product updated successfully."),
            Err(e) => Err(format!("Error updating product: {e}")),
        }
    }

    #[allow(private_interfaces)]
    #[delete("/removeproduct/<product_name>")]
    pub(super) async fn remove_product(
        product_name: String,
        mut db: Connection<RoboDatabase>,
    ) -> Result<Json<String>, String> {
        // Perform the delete query to remove the product by name
        let result = rocket_db_pools::sqlx::query("DELETE FROM products WHERE name = ?")
            .bind(&product_name) // Bind the product name to the query
            .execute(&mut **db)
            .await;
    
        match result {
            Ok(query_result) => {
                if query_result.rows_affected() > 0 {
                    // Successfully removed
                    Ok(Json("Product removed successfully.".to_string()))
                } else {
                    // No product found with the given name
                    Err("Product not found.".to_string())
                }
            }
            Err(e) => {
                // Return an error if the query failed
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
                        Err(e) => 
                        return Err(format!("Row in product variants not found {e}")),
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
                .tag_name
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
        match validate_user(jar.get("token").map(|x| x.value()).unwrap_or("")) {
            Some(_) => {}
            None => return Err("Not logged in".into()),
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
        jar: &CookieJar<'_>,
    ) -> Result<Json<String>, String> {
        match validate_user(jar.get("token").map(|x| x.value()).unwrap_or("")) {
            Some(_) => {}
            None => return Err("Not logged in".into()),
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
    ) -> Result<Json<Vec<String>>, Status> {
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

    #[derive(Deserialize)]
    struct Address {
        address_line_1: String,
        admin_area_2: String, // City
        admin_area_1: String, // State
        postal_code: String,
        country_code: String,
    }

    #[derive(Deserialize)]
    struct Customer {
        name: String,
        address: Address,
        email: String,
        phone_number: Option<String>,
    }

    #[derive(Deserialize)]
    struct OrderedItem {
        product_id: i32,
        variant: Option<i32>,
        quantity: i32,
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
        Route::new(Method::Get, "/<path..>", FileServer::from("./product_images")),
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
                api::add_item,
                api::get_websiteinfo,
                api::update_websiteinfo,
                api::create_admin,
                api::login,
                api::logout,
                api::admin_menu,
                api::current_user,
                api::get_product_variants,
                api::get_variant_id,
                api::mod_product_variant,
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
            ],
        )
}
