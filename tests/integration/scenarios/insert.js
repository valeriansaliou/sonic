// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2019, Nikita Vilunov <nikitaoryol@gmail.com>
// License: Mozilla Public License v2.0 (MPL v2.0)

const expected_documents = {
  "conversation:1" : (
    "Batch normalization is a technique for improving the speed, " +
      "performance, and stability of artificial neural networks"
  ),

  "conversation:2" : (
    "This scratch technique is much like the transform in some ways"
  )
};

const unexpected_documents = {
  "conversation:3" : "Glissando is a glide from one pitch to another"
}

async function run(search, ingest) {
  // Ingest documents
  for (const key in expected_documents) {
    await ingest.push("messages", "default", key, expected_documents[key]);
  }

  for (const key in unexpected_documents) {
    await ingest.push("messages", "default", key, unexpected_documents[key]);
  }

  // Perform search on ingested documents
  let response = await search.query("messages", "default", "technidefefefque");

  for (const key in expected_documents) {
    if (!response.includes(key) === true) {
      throw `Expected document ${key} was not found`;
    }
  }

  for (const key in unexpected_documents) {
    if (response.includes(key) === true) {
      throw `Unexpected document ${key} was returned`;
    }
  }
}

require("../runner/runner.js")(
  "Insert & Search", run
);
