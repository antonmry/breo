import './style.css'

// Simple in-memory record storage (will be replaced with PDS later)
interface Record {
  id: string
  content: string
  createdAt: Date
  published?: boolean
}

let records: Record[] = []
let editingRecordId: string | null = null

function renderApp() {
  document.querySelector<HTMLDivElement>('#app')!.innerHTML = `
    <div class="min-h-screen bg-gradient-to-br from-blue-50 to-indigo-100">
      <!-- Header -->
      <header class="bg-white shadow-sm">
        <nav class="max-w-5xl mx-auto px-4 sm:px-6 lg:px-8 py-4">
          <div class="flex items-center">
            <span class="text-xl font-bold text-gray-900">Browser PDS</span>
          </div>
        </nav>
      </header>

      <!-- Main Content -->
      <main class="max-w-5xl mx-auto px-4 sm:px-6 lg:px-8 py-8">
        <!-- Create/Edit Record Section -->
        <div class="bg-white rounded-lg shadow-md p-6 mb-6">
          <h2 class="text-2xl font-bold text-gray-900 mb-4">
            ${editingRecordId ? 'Edit Record' : 'Create New Record'}
          </h2>
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
              >${editingRecordId ? escapeHtml(records.find(r => r.id === editingRecordId)?.content || '') : ''}</textarea>
            </div>
            <div class="flex space-x-3">
              <button 
                type="submit" 
                class="bg-indigo-600 text-white px-6 py-2 rounded-lg hover:bg-indigo-700 transition font-medium"
              >
                ${editingRecordId ? 'Update Record' : 'Create Record'}
              </button>
              ${editingRecordId ? `
                <button 
                  type="button"
                  id="cancel-edit"
                  class="bg-gray-300 text-gray-700 px-6 py-2 rounded-lg hover:bg-gray-400 transition font-medium"
                >
                  Cancel
                </button>
              ` : ''}
            </div>
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
                  <div class="flex items-center space-x-3">
                    <p class="text-sm text-gray-500">
                      ${record.createdAt.toLocaleString()}
                    </p>
                    ${record.published ? `
                      <span class="inline-flex items-center px-2 py-1 rounded-full text-xs font-medium bg-green-100 text-green-800">
                        <svg class="w-3 h-3 mr-1" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                          <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M5 13l4 4L19 7"></path>
                        </svg>
                        Published
                      </span>
                    ` : ''}
                  </div>
                </div>
                <div class="flex items-center space-x-2 ml-4">
                  <button 
                    class="edit-btn text-blue-600 hover:text-blue-800 transition"
                    data-id="${record.id}"
                    title="Edit"
                  >
                    <svg class="w-5 h-5" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                      <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M11 5H6a2 2 0 00-2 2v11a2 2 0 002 2h11a2 2 0 002-2v-5m-1.414-9.414a2 2 0 112.828 2.828L11.828 15H9v-2.828l8.586-8.586z"></path>
                    </svg>
                  </button>
                  ${!record.published ? `
                    <button 
                      class="publish-btn text-green-600 hover:text-green-800 transition"
                      data-id="${record.id}"
                      title="Publish to PDS"
                    >
                      <svg class="w-5 h-5" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                        <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M7 16a4 4 0 01-.88-7.903A5 5 0 1115.9 6L16 6a5 5 0 011 9.9M15 13l-3-3m0 0l-3 3m3-3v12"></path>
                      </svg>
                    </button>
                  ` : ''}
                  <button 
                    class="delete-btn text-red-600 hover:text-red-800 transition"
                    data-id="${record.id}"
                    title="Delete"
                  >
                    <svg class="w-5 h-5" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                      <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M19 7l-.867 12.142A2 2 0 0116.138 21H7.862a2 2 0 01-1.995-1.858L5 7m5 4v6m4-6v6m1-10V4a1 1 0 00-1-1h-4a1 1 0 00-1 1v3M4 7h16"></path>
                    </svg>
                  </button>
                </div>
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
  // Create/Edit record form
  const form = document.getElementById('create-form') as HTMLFormElement
  form?.addEventListener('submit', (e) => {
    e.preventDefault()
    const textarea = document.getElementById('record-content') as HTMLTextAreaElement
    const content = textarea.value.trim()
    
    if (content) {
      if (editingRecordId) {
        // Update existing record
        const record = records.find(r => r.id === editingRecordId)
        if (record) {
          record.content = content
          record.published = false // Reset published status when edited
        }
        editingRecordId = null
      } else {
        // Create new record
        const newRecord: Record = {
          id: crypto.randomUUID(),
          content,
          createdAt: new Date(),
          published: false
        }
        records.push(newRecord)
      }
      textarea.value = ''
      renderApp()
    }
  })

  // Cancel edit button
  const cancelBtn = document.getElementById('cancel-edit')
  cancelBtn?.addEventListener('click', () => {
    editingRecordId = null
    renderApp()
  })

  // Edit record buttons
  const editButtons = document.querySelectorAll('.edit-btn')
  editButtons.forEach(button => {
    button.addEventListener('click', () => {
      const id = (button as HTMLElement).dataset.id
      editingRecordId = id || null
      renderApp()
      // Scroll to form
      window.scrollTo({ top: 0, behavior: 'smooth' })
    })
  })

  // Publish record buttons
  const publishButtons = document.querySelectorAll('.publish-btn')
  publishButtons.forEach(button => {
    button.addEventListener('click', async () => {
      const id = (button as HTMLElement).dataset.id
      const record = records.find(r => r.id === id)
      if (record) {
        // Simulate publishing to external PDS
        // In a real implementation, this would call the PDS API
        try {
          // Mock API call
          await new Promise(resolve => setTimeout(resolve, 500))
          record.published = true
          renderApp()
          
          // Show success notification (simple alert for now)
          alert('Record published to external PDS successfully!')
        } catch (error) {
          alert('Failed to publish record. Please try again.')
        }
      }
    })
  })

  // Delete record buttons
  const deleteButtons = document.querySelectorAll('.delete-btn')
  deleteButtons.forEach(button => {
    button.addEventListener('click', () => {
      const id = (button as HTMLElement).dataset.id
      if (confirm('Are you sure you want to delete this record?')) {
        records = records.filter(r => r.id !== id)
        if (editingRecordId === id) {
          editingRecordId = null
        }
        renderApp()
      }
    })
  })
}

// Initial render
renderApp()
