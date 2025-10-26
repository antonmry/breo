import init, { 
    init_identity, 
    create_post, 
    edit_profile, 
    list_records, 
    export_for_publish,
    backup,
    restore,
    get_did 
} from 'pds-wasm';

// Helper to show status messages
function showStatus(message, type = 'info') {
    const status = document.getElementById('status');
    status.textContent = message;
    status.className = `status ${type}`;
    status.style.display = 'block';
    
    if (type !== 'error') {
        setTimeout(() => {
            status.style.display = 'none';
        }, 3000);
    }
}

// Initialize WASM and check for existing identity
async function initialize() {
    try {
        await init();
        
        // Check if identity already exists
        const existingDid = await get_did();
        if (existingDid) {
            displayIdentity(existingDid);
            showFeatures();
            loadPosts();
        }
    } catch (error) {
        showStatus(`Initialization error: ${error.message}`, 'error');
        console.error('Initialization error:', error);
    }
}

// Initialize identity
document.getElementById('initBtn').addEventListener('click', async () => {
    try {
        showStatus('Creating identity...', 'info');
        const did = await init_identity();
        displayIdentity(did);
        showFeatures();
        showStatus('Identity created successfully!', 'success');
    } catch (error) {
        showStatus(`Error: ${error.message}`, 'error');
        console.error('Error creating identity:', error);
    }
});

// Display DID
function displayIdentity(did) {
    const didEl = document.getElementById('did');
    didEl.textContent = `DID: ${did}`;
    didEl.style.display = 'block';
    document.getElementById('initBtn').style.display = 'none';
}

// Show features once identity is initialized
function showFeatures() {
    document.getElementById('profileSection').style.display = 'block';
    document.getElementById('postSection').style.display = 'block';
    document.getElementById('postsSection').style.display = 'block';
    document.getElementById('backupSection').style.display = 'block';
}

// Save profile
document.getElementById('saveProfileBtn').addEventListener('click', async () => {
    try {
        const displayName = document.getElementById('displayName').value.trim() || null;
        const description = document.getElementById('description').value.trim() || null;
        
        showStatus('Saving profile...', 'info');
        await edit_profile(displayName, description);
        showStatus('Profile saved!', 'success');
    } catch (error) {
        showStatus(`Error: ${error.message}`, 'error');
        console.error('Error saving profile:', error);
    }
});

// Create post
document.getElementById('createPostBtn').addEventListener('click', async () => {
    try {
        const text = document.getElementById('postText').value;
        if (!text.trim()) {
            showStatus('Please enter some text', 'error');
            return;
        }
        
        showStatus('Creating post...', 'info');
        // Second parameter is reply_to (null for top-level posts)
        const NO_REPLY = null;
        await create_post(text, NO_REPLY);
        document.getElementById('postText').value = '';
        showStatus('Post created!', 'success');
        loadPosts();
    } catch (error) {
        showStatus(`Error: ${error.message}`, 'error');
        console.error('Error creating post:', error);
    }
});

// Load and display posts
async function loadPosts() {
    try {
        const recordsJson = await list_records('app.bsky.feed.post');
        const records = JSON.parse(recordsJson);
        
        const postsList = document.getElementById('postsList');
        if (records.length === 0) {
            postsList.innerHTML = '<p style="color: #666;">No posts yet. Create your first post!</p>';
            return;
        }
        
        postsList.innerHTML = records.map(record => `
            <div class="post">
                <div class="post-text">${escapeHtml(record.value.text)}</div>
                <div class="post-meta">
                    ${new Date(record.timestamp).toLocaleString()}
                </div>
            </div>
        `).join('');
    } catch (error) {
        console.error('Error loading posts:', error);
        document.getElementById('postsList').innerHTML = 
            '<p style="color: #721c24;">Error loading posts</p>';
    }
}

// Refresh posts
document.getElementById('refreshPostsBtn').addEventListener('click', loadPosts);

// Backup
document.getElementById('backupBtn').addEventListener('click', async () => {
    try {
        showStatus('Creating backup...', 'info');
        const backupJson = await backup();
        
        // Download as file
        const blob = new Blob([backupJson], { type: 'application/json' });
        const url = URL.createObjectURL(blob);
        const a = document.createElement('a');
        a.href = url;
        a.download = `pds-backup-${Date.now()}.json`;
        a.click();
        URL.revokeObjectURL(url);
        
        showStatus('Backup downloaded!', 'success');
    } catch (error) {
        showStatus(`Error: ${error.message}`, 'error');
        console.error('Error creating backup:', error);
    }
});

// Restore
document.getElementById('restoreBtn').addEventListener('click', () => {
    document.getElementById('restoreFile').click();
});

document.getElementById('restoreFile').addEventListener('change', async (e) => {
    const file = e.target.files[0];
    if (!file) return;
    
    try {
        showStatus('Restoring backup...', 'info');
        const text = await file.text();
        await restore(text);
        showStatus('Backup restored! Reloading...', 'success');
        
        // Reload page to refresh state
        setTimeout(() => location.reload(), 1500);
    } catch (error) {
        showStatus(`Error: ${error.message}`, 'error');
        console.error('Error restoring backup:', error);
    }
});

// Helper to escape HTML
function escapeHtml(text) {
    const div = document.createElement('div');
    div.textContent = text;
    return div.innerHTML;
}

// Initialize on page load
initialize();
