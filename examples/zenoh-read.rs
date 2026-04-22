// Use zenoh to read the robot's information. Publishing won't be added until a reliable way of setting different sesison ids is found.
// Robots key are all under `hexfellow/controllers`. A single controller will look like `hexfellow/controllers/<CPU ID>`
// You can find its config, ip address, backend git version, etc under it. A single controller might contain multiple robots.
// E.g. `hexfellow/controllers/xxxx/robots/1/api-up`

#[tokio::main]
async fn main() {}
