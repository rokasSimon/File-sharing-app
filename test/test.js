const os = require("os");
const path = require("path");
const { spawn, spawnSync } = require("child_process");
const { Builder, By, until, Capabilities } = require("selenium-webdriver");
const { expect } = require("chai");

// create the path to the expected application binary
const application = path.resolve(
  __dirname,
  "..",
  "src-tauri",
  "target",
  "release",
  "file-sharing-app.exe"
);

// keep track of the webdriver instance we create
let driver;

// keep track of the tauri-driver process we start
let tauriDriver;

before(async function () {
  // set timeout to 2 minutes
  // to allow the program to build if it needs to
  this.timeout(120000);

  // ensure the program has been built
  spawnSync("cargo", ["build", "--release"]);

  // start tauri-driver
  tauriDriver = spawn(
    path.resolve(os.homedir(), ".cargo", "bin", "tauri-driver"),
    [],
    { stdio: [null, process.stdout, process.stderr] }
  );

  const capabilities = new Capabilities();
  capabilities.set("tauri:options", { application });
  capabilities.setBrowserName("wry");

  // start the webdriver client
  driver = await new Builder()
    .withCapabilities(capabilities)
    .usingServer("http://localhost:4444/")
    .build();
});

describe("Start share directory", function() {
  this.timeout(10000);
  
  it("should create a new directory named '---Test---'", async function() {
    
    await driver.findElement(By.id("start-share-directory-btn")).click();

    let input = await driver.findElement(By.id("share-directory-name"));
    await input.click();
    await input.sendKeys("---Test---");

    let btn = await driver.wait(until.elementLocated(By.id("create-btn")), 10000);
    await btn.click();

    let directoryText = await driver.wait(until.elementLocated(By.id("---Test---")), 10000).getText();

    expect(directoryText).to.include("---Test---");
  });

  it("should leave a directory named '---Test---'", async function() {
    
    await driver.findElement(By.id("---Test---"));
    await driver.findElement(By.id("expand---Test---")).click();
    await driver.findElement(By.id("directory-leave-btn")).click();
    await driver.findElement(By.id("leave-btn")).click();

    await driver.sleep(3000);

    let items = await driver.findElements(By.id("---Test---"));

    expect(items.length).to.be.lessThan(1);
  });
});

after(async function () {
  // stop the webdriver session
  await driver.quit();

  // kill the tauri-driver process
  tauriDriver.kill();
});
