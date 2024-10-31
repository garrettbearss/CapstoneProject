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
    use rocket::http::Status;
    use sha2::{Sha256, Digest};
    use uuid::Uuid;
    use chrono::{Duration, NaiveDate, NaiveDateTime, Utc};
    use rocket::http::CookieJar;
    use rocket::http::Cookie;
    use rocket::fs::NamedFile;
    use rand::{distributions::Alphanumeric, Rng};

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

    #[derive(Serialize)]
    struct CurrentUserResponse{
        username: String
    }

    #[allow(private_interfaces)]
    #[get("/current_user")]
    pub async fn current_user(jar: &CookieJar<'_>, mut db: Connection<RoboDatabase>) -> Option<Json<CurrentUserResponse>> {
        // Get the token from the cookie jar
        if let Some(token_cookie) = jar.get("token") {
            let token_value = token_cookie.value();

            // Query the database to get the username for the token
            if let Ok(row) = rocket_db_pools::sqlx::query("SELECT username FROM admins WHERE token = ?")
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


    #[allow(private_interfaces)]
    #[post("/login", data = "<login_form>")]
    pub async fn login(login_form: Form<LoginCredentials>,mut db: Connection<RoboDatabase>,jar: &CookieJar<'_>) -> Result<Redirect, String> {
        // Fetch the admin from the database using the provided username
        let row = rocket_db_pools::sqlx::query("SELECT * FROM admins WHERE username = ?")
            .bind(&login_form.username)
            .fetch_one(&mut **db)
            .await
            .map_err(|e| e.to_string())?;

        let hashed_password = row.try_get::<String, _>("password").map_err(|e| e.to_string())?;
        let salt: String = row.try_get("salt").map_err(|e| e.to_string())?; // Fetch the salt from the database
        let expiration_str: String = row.try_get("expiration").map_err(|e| e.to_string())?;

        // Parse the expiration date from the string in "YYYY-MM-DD" format
        let expiration_date = NaiveDate::parse_from_str(&expiration_str, "%Y-%m-%d")
            .map_err(|e| e.to_string())?;
        
        // Get the current UTC date
        let now = Utc::now().naive_utc().date(); 

        // Check if the admin's expiration date has passed
        if now > expiration_date {
            // Remove the expired admin account from the database
            rocket_db_pools::sqlx::query("DELETE FROM admins WHERE username = ?")
                .bind(&login_form.username)
                .execute(&mut **db)
                .await
                .map_err(|e| e.to_string())?;
            
            return Err("Admin account has expired and has been removed.".into());
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
            rocket_db_pools::sqlx::query("UPDATE admins SET token = ?, token_expiration = ? WHERE username = ?")
                .bind(&token)
                .bind(expiration_string)
                .bind(&login_form.username)
                .execute(&mut **db)
                .await
                .map_err(|e| e.to_string())?;

            // Store the token in a cookie
            jar.add(Cookie::new("token", token));
            Ok(Redirect::to("/adminmenu.html"))
        } else {
            Err("Invalid username or password.".into())
        }
    }


    #[get("/adminmenu.html")]
    pub async fn admin_menu(jar: &CookieJar<'_>, mut db: Connection<RoboDatabase>) -> Result<NamedFile, Redirect> {
        let token = jar.get("token").map(|c| c.value().to_string());

        if let Some(token_value) = token.clone() { // Clone `token_value` here so we can reuse it later
            // Validate the token
            let user = rocket_db_pools::sqlx::query("SELECT * FROM admins WHERE token = ?")
                .bind(&token_value)
                .fetch_one(&mut **db)
                .await;

            if let Ok(row) = user {
                // Fetch `token_expiration` as a `String` from the row
                let token_expiration_str: String = match row.try_get("token_expiration") {
                    Ok(expiration) => expiration,
                    Err(_) => return Err(Redirect::to("/login")), // Redirect on error
                };

                // Parse the token expiration string into NaiveDateTime
                let token_expires = match NaiveDateTime::parse_from_str(&token_expiration_str, "%Y-%m-%d %H:%M:%S") {
                    Ok(parsed_date) => parsed_date,
                    Err(_) => return Err(Redirect::to("/login")), // Redirect if parsing fails
                };

                let now = Utc::now().naive_utc(); // Get the current time in naive UTC

                // Check if the token has expired
                if token_expires > now {
                    return NamedFile::open("./pages/adminmenu.html").await.map_err(|_| Redirect::to("/api/login"));
                } else {
                    // If the token is expired, clear it from the database
                    rocket_db_pools::sqlx::query("UPDATE admins SET token = NULL, token_expires = NULL WHERE token = ?")
                        .bind(token_value) // Use the cloned value here
                        .execute(&mut **db)
                        .await
                        .ok();
                }
            }
        }
        Err(Redirect::to("api/login"))
    }


    #[post("/logout")]
    pub async fn logout(jar: &CookieJar<'_>, mut db: Connection<RoboDatabase>){
        // Get the token from the cookie
        if let Some(token_cookie) = jar.get("token") {
            let token_value = token_cookie.value().to_string();

            // Remove the "token" cookie from the jar to log out the client
            jar.remove(Cookie::from("token"));

            // Set the token and token_expires fields to NULL for the user in the database
            let update_result = rocket_db_pools::sqlx::query("UPDATE admins SET token = NULL, token_expiration = NULL WHERE token = ?")
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
    pub async fn create_admin(admin_form: Form<CreateAdmin>,mut db: Connection<RoboDatabase>) -> Result<Redirect, Status> {
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
            "INSERT INTO admins (username, salt, password, expiration) VALUES (?, ?, ?, ?)"
        )
        .bind(&admin_form.username)
        .bind(salt)
        .bind(hashed_password)
        .bind(&admin_form.expiration)
        .execute(&mut **db)
        .await;

        // Handle the result of the database operation
        match result {
            Ok(_) => Ok(Redirect::to("/adminconfirm.html")), // Redirect to confirmation page
            Err(_) => Err(Status::InternalServerError),
        }
    }

    fn hash_password(password: &str) -> String {
        // Hashing logic using SHA-256
        let mut hasher = Sha256::new();
        hasher.update(password);
        let result = hasher.finalize();
        hex::encode(result)  // Return the hex representation of the hash
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
        Ok(_) => Ok(Redirect::to("/adminconfirm.html")),
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
        .mount("/api", routes![
            api::get_items, 
            api::add_item, 
            api::get_websiteinfo, 
            api::update_websiteinfo, 
            api::create_admin, 
            api::login, 
            api::logout, 
            api::admin_menu, 
            api::current_user])
}
