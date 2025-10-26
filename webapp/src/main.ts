import './style.css';
import init, { WasmRepository } from '../../crates/wasm/pkg/pds_wasm.js';

// Global repository instance
let repo: WasmRepository | null = null;

// Initialize WASM and repository
async function initializeApp() {
  try {
    await init();
    repo = new WasmRepository();
    console.log('WASM initialized successfully');
    
    // Update UI state
    updateUIState();
  } catch (err) {
    console.error('Failed to initialize WASM:', err);
    showError('Failed to initialize application: ' + err);
  }
}

// Update UI based on repository state
function updateUIState() {
  const did = repo?.get_did();
  const initSection = document.getElementById('init-section');
  const mainSections = document.getElementById('main-sections');
  
  if (did) {
    initSection?.classList.add('hidden');
    mainSections?.classList.remove('hidden');
    
    // Update identity info
    const didDisplay = document.getElementById('did-display');
    const pubKeyDisplay = document.getElementById('pubkey-display');
    
    if (didDisplay) didDisplay.textContent = did;
    if (pubKeyDisplay) {
      const pubKey = repo?.get_public_key();
      pubKeyDisplay.textContent = pubKey ? pubKey.substring(0, 40) + '...' : 'N/A';
    }
  } else {
    initSection?.classList.remove('hidden');
    mainSections?.classList.add('hidden');
  }
}

// Initialize repository with DID
async function initRepository() {
  const didInput = document.getElementById('did-input') as HTMLInputElement;
  const did = didInput.value.trim();
  
  if (!did) {
    showError('Please enter a DID');
    return;
  }
  
  try {
    await repo?.init_identity(did);
    showSuccess('Repository initialized successfully!');
    updateUIState();
  } catch (err) {
    showError('Failed to initialize repository: ' + err);
  }
}

// Create a new post
async function createPost() {
  const textArea = document.getElementById('post-text') as HTMLTextAreaElement;
  const text = textArea.value.trim();
  
  if (!text) {
    showError('Please enter post text');
    return;
  }
  
  try {
    const cid = await repo?.create_post(text);
    showSuccess('Post created successfully! CID: ' + cid);
    textArea.value = '';
    
    // Refresh feed
    await loadFeed();
  } catch (err) {
    showError('Failed to create post: ' + err);
  }
}

// Update profile
async function updateProfile() {
  const displayName = (document.getElementById('display-name') as HTMLInputElement).value.trim();
  const bio = (document.getElementById('bio') as HTMLTextAreaElement).value.trim();
  
  try {
    const cid = await repo?.edit_profile(displayName, bio);
    showSuccess('Profile updated successfully! CID: ' + cid);
  } catch (err) {
    showError('Failed to update profile: ' + err);
  }
}

// Load and display feed
async function loadFeed() {
  try {
    const records = repo?.list_records('app.bsky.feed.post');
    let posts;
    try {
      posts = JSON.parse(records || '[]');
    } catch (parseError) {
      console.error('Failed to parse feed records:', parseError);
      showError('Failed to parse feed data');
      return;
    }
    
    const feedContainer = document.getElementById('feed-container');
    if (!feedContainer) return;
    
    if (posts.length === 0) {
      feedContainer.innerHTML = '<p class="text-gray-500 text-center py-4">No posts yet</p>';
      return;
    }
    
    feedContainer.innerHTML = posts
      .reverse() // Show newest first
      .map((post: any) => `
        <div class="bg-white p-4 rounded-lg shadow mb-3 border border-gray-200">
          <p class="text-gray-800">${escapeHtml(post.value.text)}</p>
          <p class="text-xs text-gray-500 mt-2">${new Date(post.value.createdAt).toLocaleString()}</p>
        </div>
      `)
      .join('');
  } catch (err) {
    showError('Failed to load feed: ' + err);
  }
}

// Export identity (DID and public key)
function exportIdentity() {
  const did = repo?.get_did();
  const pubKey = repo?.get_public_key();
  
  if (!did || !pubKey) {
    showError('Repository not initialized');
    return;
  }
  
  const identity = {
    did,
    publicKey: pubKey,
    warning: 'Keep this information secure. The private key is stored in your browser.'
  };
  
  const blob = new Blob([JSON.stringify(identity, null, 2)], { type: 'application/json' });
  downloadBlob(blob, 'pds-identity.json');
  
  showSuccess('Identity exported successfully');
}

// Create backup
function createBackup() {
  try {
    const backup = repo?.backup();
    if (!backup) {
      showError('Failed to create backup');
      return;
    }
    
    const blob = new Blob([backup], { type: 'application/json' });
    downloadBlob(blob, `pds-backup-${Date.now()}.json`);
    
    showSuccess('Backup created successfully');
  } catch (err) {
    showError('Failed to create backup: ' + err);
  }
}

// Restore from backup
async function restoreBackup() {
  const fileInput = document.getElementById('restore-file') as HTMLInputElement;
  const file = fileInput.files?.[0];
  
  if (!file) {
    showError('Please select a backup file');
    return;
  }
  
  try {
    const text = await file.text();
    await repo?.restore(text);
    showSuccess('Backup restored successfully!');
    updateUIState();
    await loadFeed();
  } catch (err) {
    showError('Failed to restore backup: ' + err);
  }
}

// Show publish dialog
function showPublishDialog() {
  const dialog = document.getElementById('publish-dialog');
  dialog?.classList.remove('hidden');
}

// Hide publish dialog
function hidePublishDialog() {
  const dialog = document.getElementById('publish-dialog');
  dialog?.classList.add('hidden');
}

// Publish to remote PDS
async function publishToRemote() {
  const urlInput = document.getElementById('remote-pds-url') as HTMLInputElement;
  const url = urlInput.value.trim();
  
  if (!url) {
    showError('Please enter a remote PDS URL');
    return;
  }
  
  try {
    const snapshot = repo?.export_for_publish();
    if (!snapshot) {
      showError('Failed to export snapshot');
      return;
    }
    
    // TODO: Implement actual ATProto publishing via fetch() to remote PDS endpoints
    // This would require:
    // 1. Converting snapshot to CAR format
    // 2. Proper ATProto authentication
    // 3. API endpoints for createRecord, etc.
    showInfo(`Publishing to ${url}...\n\nNote: Remote publishing requires proper ATProto endpoint implementation.\nSnapshot ready for export.`);
    
    // Optionally download the snapshot for manual upload
    const blob = new Blob([snapshot], { type: 'application/json' });
    downloadBlob(blob, 'pds-snapshot.json');
    
    hidePublishDialog();
  } catch (err) {
    showError('Failed to publish: ' + err);
  }
}

// Helper: Download blob
function downloadBlob(blob: Blob, filename: string) {
  const url = URL.createObjectURL(blob);
  const a = document.createElement('a');
  a.href = url;
  a.download = filename;
  a.click();
  URL.revokeObjectURL(url);
}

// Helper: Escape HTML
function escapeHtml(text: string): string {
  const div = document.createElement('div');
  div.textContent = text;
  return div.innerHTML;
}

// Helper: Show success message
function showSuccess(message: string) {
  showToast(message, 'success');
}

// Helper: Show error message
function showError(message: string) {
  showToast(message, 'error');
}

// Helper: Show info message
function showInfo(message: string) {
  showToast(message, 'info');
}

// Helper: Show toast notification
function showToast(message: string, type: 'success' | 'error' | 'info') {
  const toast = document.createElement('div');
  const bgColor = type === 'success' ? 'bg-green-500' : type === 'error' ? 'bg-red-500' : 'bg-blue-500';
  
  toast.className = `fixed top-4 right-4 ${bgColor} text-white px-6 py-3 rounded-lg shadow-lg z-50 max-w-md`;
  toast.textContent = message;
  
  document.body.appendChild(toast);
  
  setTimeout(() => {
    toast.remove();
  }, 5000);
}

// Render the UI
function renderUI() {
  const app = document.getElementById('app');
  if (!app) return;
  
  app.innerHTML = `
    <div class="min-h-screen bg-gray-50">
      <!-- Header -->
      <header class="bg-blue-600 text-white shadow-md">
        <div class="container mx-auto px-4 py-6">
          <h1 class="text-3xl font-bold">🌐 Browser PDS</h1>
          <p class="text-blue-100 mt-1">Local-First ATProto Personal Data Server</p>
        </div>
      </header>
      
      <!-- Main Content -->
      <main class="container mx-auto px-4 py-8 max-w-4xl">
        
        <!-- Initialization Section -->
        <div id="init-section" class="bg-white rounded-lg shadow-md p-6">
          <h2 class="text-2xl font-semibold mb-4">Initialize Your Repository</h2>
          <p class="text-gray-600 mb-4">Enter a DID to create or connect to your personal data repository.</p>
          
          <input 
            type="text" 
            id="did-input" 
            placeholder="did:plc:example123" 
            value="did:plc:alice"
            class="w-full px-4 py-2 border border-gray-300 rounded-md mb-4 focus:ring-2 focus:ring-blue-500 focus:border-transparent"
          />
          
          <button 
            onclick="window.initRepo()" 
            class="bg-blue-600 hover:bg-blue-700 text-white px-6 py-2 rounded-md font-medium transition"
          >
            Initialize Repository
          </button>
        </div>
        
        <!-- Main Sections (hidden until initialized) -->
        <div id="main-sections" class="hidden space-y-6">
          
          <!-- Identity Section -->
          <div class="bg-white rounded-lg shadow-md p-6">
            <h2 class="text-2xl font-semibold mb-4">🔑 Identity</h2>
            
            <div class="space-y-3">
              <div>
                <label class="text-sm font-medium text-gray-700">DID</label>
                <p id="did-display" class="text-gray-900 font-mono bg-gray-50 p-2 rounded mt-1"></p>
              </div>
              
              <div>
                <label class="text-sm font-medium text-gray-700">Public Key</label>
                <p id="pubkey-display" class="text-gray-900 font-mono bg-gray-50 p-2 rounded mt-1 text-sm"></p>
              </div>
              
              <button 
                onclick="window.exportIdentity()" 
                class="bg-purple-600 hover:bg-purple-700 text-white px-4 py-2 rounded-md font-medium transition text-sm"
              >
                Export Identity
              </button>
            </div>
          </div>
          
          <!-- Profile Editor Section -->
          <div class="bg-white rounded-lg shadow-md p-6">
            <h2 class="text-2xl font-semibold mb-4">👤 Profile Editor</h2>
            <p class="text-sm text-gray-600 mb-4">Automerge-backed profile document</p>
            
            <div class="space-y-4">
              <div>
                <label class="block text-sm font-medium text-gray-700 mb-1">Display Name</label>
                <input 
                  type="text" 
                  id="display-name" 
                  placeholder="Your Name" 
                  value="Alice"
                  class="w-full px-4 py-2 border border-gray-300 rounded-md focus:ring-2 focus:ring-blue-500 focus:border-transparent"
                />
              </div>
              
              <div>
                <label class="block text-sm font-medium text-gray-700 mb-1">Bio</label>
                <textarea 
                  id="bio" 
                  placeholder="Tell us about yourself..."
                  rows="3"
                  class="w-full px-4 py-2 border border-gray-300 rounded-md focus:ring-2 focus:ring-blue-500 focus:border-transparent resize-none"
                >I love decentralized protocols!</textarea>
              </div>
              
              <button 
                onclick="window.updateProfile()" 
                class="bg-green-600 hover:bg-green-700 text-white px-6 py-2 rounded-md font-medium transition"
              >
                Update Profile
              </button>
            </div>
          </div>
          
          <!-- Post Composer Section -->
          <div class="bg-white rounded-lg shadow-md p-6">
            <h2 class="text-2xl font-semibold mb-4">✍️ Post Composer</h2>
            
            <div class="space-y-4">
              <textarea 
                id="post-text" 
                placeholder="What's on your mind?"
                rows="4"
                class="w-full px-4 py-2 border border-gray-300 rounded-md focus:ring-2 focus:ring-blue-500 focus:border-transparent resize-none"
              ></textarea>
              
              <button 
                onclick="window.createPost()" 
                class="bg-blue-600 hover:bg-blue-700 text-white px-6 py-2 rounded-md font-medium transition"
              >
                Create Post
              </button>
            </div>
          </div>
          
          <!-- Feed Viewer Section -->
          <div class="bg-white rounded-lg shadow-md p-6">
            <div class="flex justify-between items-center mb-4">
              <h2 class="text-2xl font-semibold">📰 Feed</h2>
              <button 
                onclick="window.loadFeed()" 
                class="text-blue-600 hover:text-blue-700 text-sm font-medium"
              >
                Refresh
              </button>
            </div>
            
            <div id="feed-container" class="space-y-3">
              <p class="text-gray-500 text-center py-4">No posts yet</p>
            </div>
          </div>
          
          <!-- Publish & Backup Section -->
          <div class="bg-white rounded-lg shadow-md p-6">
            <h2 class="text-2xl font-semibold mb-4">☁️ Publish & Backup</h2>
            
            <div class="space-y-4">
              <!-- Publish -->
              <div>
                <h3 class="font-medium mb-2">Publish to Remote PDS</h3>
                <button 
                  onclick="window.showPublish()" 
                  class="bg-indigo-600 hover:bg-indigo-700 text-white px-4 py-2 rounded-md font-medium transition"
                >
                  Publish...
                </button>
              </div>
              
              <!-- Backup -->
              <div>
                <h3 class="font-medium mb-2">Backup & Restore</h3>
                <div class="bg-yellow-50 border border-yellow-200 rounded-md p-3 mb-3">
                  <p class="text-sm text-yellow-800">
                    ⚠️ <strong>Important:</strong> Regular backups are mandatory! Your data is stored locally in IndexedDB and can be cleared by the browser.
                  </p>
                </div>
                
                <div class="flex gap-3">
                  <button 
                    onclick="window.createBackup()" 
                    class="bg-orange-600 hover:bg-orange-700 text-white px-4 py-2 rounded-md font-medium transition"
                  >
                    Create Backup
                  </button>
                  
                  <label class="bg-gray-600 hover:bg-gray-700 text-white px-4 py-2 rounded-md font-medium transition cursor-pointer">
                    Restore Backup
                    <input 
                      type="file" 
                      id="restore-file" 
                      accept=".json" 
                      onchange="window.restoreBackup()"
                      class="hidden"
                    />
                  </label>
                </div>
              </div>
            </div>
          </div>
          
        </div>
      </main>
      
      <!-- Publish Dialog -->
      <div id="publish-dialog" class="hidden fixed inset-0 bg-black bg-opacity-50 flex items-center justify-center z-40">
        <div class="bg-white rounded-lg shadow-xl p-6 max-w-md w-full mx-4">
          <h3 class="text-xl font-semibold mb-4">Publish to Remote PDS</h3>
          
          <div class="mb-4">
            <label class="block text-sm font-medium text-gray-700 mb-2">Remote PDS URL</label>
            <input 
              type="url" 
              id="remote-pds-url" 
              placeholder="https://bsky.social" 
              class="w-full px-4 py-2 border border-gray-300 rounded-md focus:ring-2 focus:ring-blue-500 focus:border-transparent"
            />
          </div>
          
          <div class="bg-blue-50 border border-blue-200 rounded-md p-3 mb-4">
            <p class="text-sm text-blue-800">
              ℹ️ Publishing will export your repository snapshot. Actual remote sync requires ATProto endpoint implementation.
            </p>
          </div>
          
          <div class="flex gap-3 justify-end">
            <button 
              onclick="window.hidePublish()" 
              class="px-4 py-2 text-gray-700 hover:bg-gray-100 rounded-md font-medium transition"
            >
              Cancel
            </button>
            <button 
              onclick="window.publishRemote()" 
              class="bg-indigo-600 hover:bg-indigo-700 text-white px-4 py-2 rounded-md font-medium transition"
            >
              Publish
            </button>
          </div>
        </div>
      </div>
    </div>
  `;
  
  // Attach event handlers to window for inline onclick handlers
  (window as any).initRepo = initRepository;
  (window as any).createPost = createPost;
  (window as any).updateProfile = updateProfile;
  (window as any).loadFeed = loadFeed;
  (window as any).exportIdentity = exportIdentity;
  (window as any).createBackup = createBackup;
  (window as any).restoreBackup = restoreBackup;
  (window as any).showPublish = showPublishDialog;
  (window as any).hidePublish = hidePublishDialog;
  (window as any).publishRemote = publishToRemote;
}

// Initialize on load
document.addEventListener('DOMContentLoaded', async () => {
  renderUI();
  await initializeApp();
});
