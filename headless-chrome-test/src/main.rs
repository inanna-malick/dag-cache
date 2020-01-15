use std::io::Write;
use headless_chrome::{Browser, protocol::page::ScreenshotFormat};


// navigate to fresh page, make modification to head node, wait for save, take screenshot
fn stage_1(b: &Browser) {
    let tab = b.wait_for_initial_tab().unwrap();
    tab.navigate_to("http://localhost:3030").expect("navigate-to");

    tab.wait_for_element(".node-header").unwrap().click().unwrap();
    tab.type_str("test header").unwrap().press_key("Enter").unwrap();

    // interval between saves
    let ten_millis = std::time::Duration::from_secs(10);
    std::thread::sleep(ten_millis);

    tab.wait_for_element(".state-is-unmodified").unwrap();

    // Take a screenshot of the entire browser window
    let png = tab.capture_screenshot(
        ScreenshotFormat::PNG,
        None,
        true).unwrap();

    let mut file = std::fs::File::create("notes-app-stage-1.png").unwrap();
    file.write_all(&png).unwrap();
}


// navigate to modified page, expand via load node, take screenshot
fn stage_2(b: &Browser) {
    let tab = b.wait_for_initial_tab().unwrap();
    tab.navigate_to("http://localhost:3030").expect("navigate-to");

    // load node then wait for header to materialize
    tab.wait_for_element(".load-node").unwrap().click().unwrap();
    tab.wait_for_element(".node-header").unwrap();

    // Take a screenshot of the entire browser window
    let png = tab.capture_screenshot(
        ScreenshotFormat::PNG,
        None,
        true).unwrap();

    let mut file = std::fs::File::create("notes-app-stage-2.png").unwrap();
    file.write_all(&png).unwrap();
}


fn main() {
    let b = Browser::default().unwrap();
    println!("init test");
    stage_1(&b);
    println!("stage 1 done");
    stage_2(&b);
    println!("stage 2 done");
}
