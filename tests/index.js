var SonicChannelSearch = require("sonic-channel").Search;

function createSearch() {
    return new Promise((resolve, reject) => {
        let chan = new SonicChannelSearch({
            host: "localhost", port: 1491, auth: "SecretPassword"
        });
        chan.connect({
            connected() {
                console.info("Sonic Channel succeeded to connect to host (search).");
                resolve(chan);
            },
            disconnected() {
                console.error("Sonic Channel is now disconnected (search).");
            },
            timeout() {
                console.error("Sonic Channel connection timed out (search).");
            },
            retrying() {
                console.error("Trying to reconnect to Sonic Channel (search)...");
            },
            error(error) {
                console.error("Sonic Channel failed to connect to host (search).", error);
                reject(error);
            }
        });
    });
}

async function runTests(search) {
    let pong = await search.ping();
    console.log("Pong");
};

async function main() {
    let search = await createSearch();
    await runTests(search);
    await search.close();
}

main().then(
    () => {},
    (err)  => { 
        console.log(err);
        process.exit(-1);
    }
);
