import './style.css'

// Simple in-memory record storage (will be replaced with PDS later)
interface Record {
  id: string
  content: string
  createdAt: Date
}

let records: Record[] = []

function renderApp() {
  document.querySelector<HTMLDivElement>('#app')!.innerHTML = `
    <div class="min-h-screen bg-gradient-to-br from-blue-50 to-indigo-100">
      <!-- Header -->
      <header class="bg-white shadow-sm">
        <nav class="max-w-5xl mx-auto px-4 sm:px-6 lg:px-8 py-4">
          <div class="flex items-center space-x-2">
            <svg class="w-6 h-6 text-indigo-600" fill="none" stroke="currentColor" viewBox="0 0 24 24">
              <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M5 12h14M5 12a2 2 0 01-2-2V6a2 2 0 012-2h14a2 2 0 012 2v4a2 2 0 01-2 2M5 12a2 2 0 00-2 2v4a2 2 0 002 2h14a2 2 0 002-2v-4a2 2 0 00-2-2m-2-4h.01M17 16h.01"></path>
            </svg>
            <span class="text-xl font-bold text-gray-900">Browser PDS</span>
          </div>
        </nav>
      </header>

      <!-- Main Content -->
      <main class="max-w-5xl mx-auto px-4 sm:px-6 lg:px-8 py-8">
        <!-- Create Record Section -->
        <div class="bg-white rounded-lg shadow-md p-6 mb-6">
          <h2 class="text-2xl font-bold text-gray-900 mb-4">Create New Record</h2>
          <form id="create-form" class="space-y-4">
            <div>
              <label for="record-content" class="block text-sm font-medium text-gray-700 mb-2">
                Content
              </label>
              <textarea 
                id="record-content" 
                rows="3" 
                class="w-full px-4 py-2 border border-gray-300 rounded-lg focus:ring-2 focus:ring-indigo-500 focus:border-transparent"
                placeholder="Enter your record content..."
                required
              ></textarea>
            </div>
            <button 
              type="submit" 
              class="bg-indigo-600 text-white px-6 py-2 rounded-lg hover:bg-indigo-700 transition font-medium"
            >
              Create Record
            </button>
          </form>
        </div>

        <!-- Records List Section -->
        <div class="bg-white rounded-lg shadow-md p-6">
          <h2 class="text-2xl font-bold text-gray-900 mb-4">Records</h2>
          <div id="records-list" class="space-y-3">
            ${records.length === 0 ? `
              <p class="text-gray-500 text-center py-8">No records yet. Create your first record above!</p>
            ` : records.map(record => `
              <div class="flex items-start justify-between p-4 bg-gray-50 rounded-lg hover:bg-gray-100 transition">
                <div class="flex-1">
                  <p class="text-gray-900 mb-1">${escapeHtml(record.content)}</p>
                  <p class="text-sm text-gray-500">
                    ${record.createdAt.toLocaleString()}
                  </p>
                </div>
                <button 
                  class="delete-btn ml-4 text-red-600 hover:text-red-800 transition"
                  data-id="${record.id}"
                >
                  <svg class="w-5 h-5" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                    <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M19 7l-.867 12.142A2 2 0 0116.138 21H7.862a2 2 0 01-1.995-1.858L5 7m5 4v6m4-6v6m1-10V4a1 1 0 00-1-1h-4a1 1 0 00-1 1v3M4 7h16"></path>
                  </svg>
                </button>
              </div>
            `).join('')}
          </div>
        </div>
      </main>
    </div>
  `

  // Attach event listeners
  attachEventListeners()
}

function escapeHtml(text: string): string {
  const div = document.createElement('div')
  div.textContent = text
  return div.innerHTML
}

function attachEventListeners() {
  // Create record form
  const form = document.getElementById('create-form') as HTMLFormElement
  form?.addEventListener('submit', (e) => {
    e.preventDefault()
    const textarea = document.getElementById('record-content') as HTMLTextAreaElement
    const content = textarea.value.trim()
    
    if (content) {
      const newRecord: Record = {
        id: crypto.randomUUID(),
        content,
        createdAt: new Date()
      }
      records.push(newRecord)
      textarea.value = ''
      renderApp()
    }
  })

  // Delete record buttons
  const deleteButtons = document.querySelectorAll('.delete-btn')
  deleteButtons.forEach(button => {
    button.addEventListener('click', () => {
      const id = (button as HTMLElement).dataset.id
      records = records.filter(r => r.id !== id)
      renderApp()
    })
  })
}

// Initial render
renderApp()
