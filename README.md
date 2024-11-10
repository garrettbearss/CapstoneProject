# Capstone Project - Kent State Robotics Merch Site

## Overview
The Kent State Robotics Merch Site is a web application designed for the Kent State Robotics Club. It provides a platform for the club to sell merchandise, accept donations, and manage product inventory and orders through an admin interface. The site also includes color-blind accessibility features, and payment integrations via PayPal.

## Table of Contents
- [Project Objectives](#project-objectives)
- [Features](#features)
- [Technologies Used](#technologies-used)
- [Installation](#installation)
- [Usage](#usage)
- [Admin Interface](#admin-interface)

## Project Objectives
The primary goals of the project are:
1. Provide an e-commerce platform to sell Kent State Robotics Club merchandise.
2. Integrate a secure payment system to handle transactions.
3. Allow administrators to manage products sold, view and access order history, modify information displayed on the website.
4. Ensure accessibility, including a toggle for color-blind users.
5. Promote donations to support the club’s projects and initiatives.

## Features
- **Product Display**: View available items with descriptions, prices, and images.
- **Shopping Cart**: Add or remove products, view cart total, and proceed to checkout.
- **Checkout with PayPal**: Secure payment processing using PayPal.
- **Donation Page**: Accept donations with custom amounts.
- **Admin Dashboard**: Access to product management, order overview, and website information.
- **Color-Blind Mode**: Toggle visual settings for improved accessibility.
  
## Technologies Used
- **Frontend**: HTML, CSS, JavaScript
- **Backend**: Rust (using Rocket framework for API endpoints)
- **Database**: SQLite
- **Payment Processing**: PayPal SDK

## Installation

1. **Clone the Repository**
   ```bash
   git clone https://github.com/garrettbearss/CapstoneProject.git
   cd CapstoneProject
   
2. **Backend Setup**
   - Install [Rust](https://www.rust-lang.org/tools/install) and ensure it's up-to-date.
   - Install the Rocket framework:
     ```bash
     cargo install rocket
     ```
   - Configure the database (SQLite) in the `.env` file, providing database credentials if needed:
     ```
     DATABASE_URL=sqlite://./database.db
     ```
   - Set up environment variables for PayPal client ID and secret:
     ```
     PAYPAL_CLIENT_ID=your_paypal_client_id
     PAYPAL_SECRET=your_paypal_secret
     ```
   - Run migrations if applicable and start the server:
     ```bash
     cargo run
     ```

3. **Frontend Setup**
   - No additional setup is needed for the frontend as it is designed to run on the same server.

4. **Environment Variables**
   - The `Rocket.toml` file should include essential environment variables like the database URL and PayPal credentials.

## Usage

1. **Accessing the Site**
   - Launch the application locally by navigating to `http://localhost:8000` in your browser.

2. **Product Browsing and Checkout**
   - Browse available products on the products page and add items to the shopping cart.
   - Go to the checkout page to review the cart and proceed with the PayPal checkout.

3. **Donations**
   - Navigate to the `donate.html` page to contribute custom amounts to support the club.

4. **Color-Blind Accessibility Toggle**
   - Use the toggle button in the navbar to activate color-blind mode, designed for improved accessibility.

## Admin Interface

1. **Admin Login**
   - Admins can log in through the `adminlogin.html` page.
   - Once logged in, admins will gain access to additional features within the website.

2. **Product Management**
   - Admins can add, update, or remove products displayed on the main site.
   - Product listings can include a name, description, price, and image.

3. **Order Management**
   - View recent orders, update order status, and access historical order data.

4. **Admin Creation**
   - Create new expiring admin users using a username and password

5. **Website Information Management**
   - Change information displayed on the `aboutus.html` page.
