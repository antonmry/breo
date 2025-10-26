//! Example demonstrating the core ATProto repository functionality
//!
//! This example shows:
//! - Creating a repository
//! - Adding records (posts)
//! - Updating and deleting records
//! - Working with the commit graph
//! - Using Automerge for mutable documents
//! - Exporting snapshots

use pds_core::{
    automerge_wrapper::AutomergeDoc,
    repo::Repository,
    snapshot::{CommitLog, RecordSnapshot, Snapshot},
    traits::{Ed25519Crypto, MemoryKvStore, SystemClock},
    types::{Did, Nsid, RecordKey},
};

fn main() -> pds_core::Result<()> {
    println!("=== PDS Core Example ===\n");

    // 1. Set up repository
    println!("1. Creating repository...");
    let did = Did::new("did:plc:alice123")?;
    let store = MemoryKvStore::new();
    let clock = SystemClock;
    let crypto = Ed25519Crypto::new();

    let mut repo = Repository::new(did, store, clock, crypto);
    println!("   Repository created for DID: {}\n", repo.did());

    // 2. Create some posts
    println!("2. Creating posts...");
    let post_collection = Nsid::new("app.bsky.feed.post")?;

    let post1 = serde_json::json!({
        "text": "Hello ATProto world!",
        "createdAt": "2025-01-01T00:00:00Z"
    });
    let rkey1 = RecordKey::new("post1");
    let cid1 = repo.create_record(post_collection.clone(), rkey1.clone(), post1)?;
    println!("   Created post1 with CID: {}", cid1);

    let post2 = serde_json::json!({
        "text": "This is my second post",
        "createdAt": "2025-01-01T01:00:00Z"
    });
    let rkey2 = RecordKey::new("post2");
    let cid2 = repo.create_record(post_collection.clone(), rkey2, post2)?;
    println!("   Created post2 with CID: {}\n", cid2);

    // 3. Update a post
    println!("3. Updating post1...");
    let updated_post = serde_json::json!({
        "text": "Hello ATProto world! (edited)",
        "createdAt": "2025-01-01T00:00:00Z",
        "edited": true
    });
    let updated_cid = repo.update_record(post_collection.clone(), rkey1.clone(), updated_post)?;
    println!("   Updated post1 with new CID: {}\n", updated_cid);

    // 4. List all posts
    println!("4. Listing all posts...");
    let posts = repo.list_records(&post_collection);
    println!("   Found {} posts:", posts.len());
    for post in &posts {
        println!("   - {}: {}", post.rkey, post.value);
    }
    println!();

    // 5. Show commit history
    println!("5. Commit history...");
    let commits = repo.get_commits()?;
    println!("   Total commits: {}", commits.len());
    for (i, commit) in commits.iter().enumerate() {
        println!(
            "   Commit {}: {:?} {} at {}",
            i + 1,
            commit.operation,
            commit.rkey,
            commit.timestamp
        );
    }
    println!();

    // 6. Demonstrate Automerge for mutable documents
    println!("6. Automerge document example...");
    let profile = serde_json::json!({
        "displayName": "Alice",
        "bio": "ATProto enthusiast",
        "avatar": "https://example.com/avatar.jpg"
    });

    let mut profile_doc = AutomergeDoc::from_json(&profile)?;
    println!("   Created profile document");

    // Simulate an update
    let updated_profile = serde_json::json!({
        "displayName": "Alice Smith",
        "bio": "ATProto enthusiast and developer",
        "avatar": "https://example.com/avatar.jpg",
        "website": "https://alice.example.com"
    });
    profile_doc.update(&updated_profile)?;
    println!("   Updated profile document");

    // Save and load
    let saved_bytes = profile_doc.save();
    let loaded_doc = AutomergeDoc::load(&saved_bytes)?;
    let final_profile = loaded_doc.to_json()?;
    println!("   Loaded profile: {}\n", final_profile);

    // 7. Export snapshot
    println!("7. Creating snapshot...");
    let snapshot = Snapshot::from_repo(&repo)?;
    println!("   Snapshot DID: {}", snapshot.did);
    println!("   Snapshot version: {}", snapshot.version);
    println!("   Commits in snapshot: {}", snapshot.commits.len());

    let snapshot_json = snapshot.to_json()?;
    println!("   Snapshot JSON size: {} bytes\n", snapshot_json.len());

    // 8. Export commit log
    println!("8. Creating commit log...");
    let commit_log = CommitLog::from_repo(&repo)?;
    let log_json = commit_log.to_json()?;
    println!("   Commit log JSON size: {} bytes\n", log_json.len());

    // 9. Export individual record
    println!("9. Creating record snapshot...");
    if let Some(record) = repo.get_record(&post_collection, &rkey1) {
        let record_snapshot = RecordSnapshot::new(record.clone())?;
        println!("   Record CID: {}", record_snapshot.cid);
        println!("   Record path: {}\n", record.path());
    }

    println!("=== Example Complete ===");

    Ok(())
}
