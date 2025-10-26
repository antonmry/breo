import './style.css'

document.querySelector<HTMLDivElement>('#app')!.innerHTML = `
  <div class="min-h-screen bg-gradient-to-br from-blue-50 to-indigo-100">
    <!-- Header -->
    <header class="bg-white shadow-sm">
      <nav class="max-w-7xl mx-auto px-4 sm:px-6 lg:px-8 py-4">
        <div class="flex justify-between items-center">
          <div class="flex items-center space-x-2">
            <svg class="w-8 h-8 text-indigo-600" fill="none" stroke="currentColor" viewBox="0 0 24 24">
              <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M5 12h14M5 12a2 2 0 01-2-2V6a2 2 0 012-2h14a2 2 0 012 2v4a2 2 0 01-2 2M5 12a2 2 0 00-2 2v4a2 2 0 002 2h14a2 2 0 002-2v-4a2 2 0 00-2-2m-2-4h.01M17 16h.01"></path>
            </svg>
            <span class="text-2xl font-bold text-gray-900">Browser PDS</span>
          </div>
          <div class="flex items-center space-x-4">
            <a href="#features" class="text-gray-600 hover:text-gray-900 transition">Features</a>
            <a href="#about" class="text-gray-600 hover:text-gray-900 transition">About</a>
            <button class="bg-indigo-600 text-white px-4 py-2 rounded-lg hover:bg-indigo-700 transition">
              Get Started
            </button>
          </div>
        </div>
      </nav>
    </header>

    <!-- Hero Section -->
    <main class="max-w-7xl mx-auto px-4 sm:px-6 lg:px-8 py-16">
      <div class="text-center mb-16">
        <h1 class="text-5xl font-bold text-gray-900 mb-4">
          Your Personal Data Server
          <span class="block text-indigo-600 mt-2">Running in Your Browser</span>
        </h1>
        <p class="text-xl text-gray-600 max-w-3xl mx-auto mb-8">
          A local-first, ATProto-compatible Personal Data Server built with Rust and WebAssembly. 
          Own your data, run it anywhere, publish when you want.
        </p>
        <div class="flex justify-center space-x-4">
          <button class="bg-indigo-600 text-white px-8 py-3 rounded-lg text-lg font-semibold hover:bg-indigo-700 transition shadow-lg">
            Launch Browser PDS
          </button>
          <button class="bg-white text-indigo-600 px-8 py-3 rounded-lg text-lg font-semibold hover:bg-gray-50 transition shadow-lg border-2 border-indigo-600">
            Learn More
          </button>
        </div>
      </div>

      <!-- Features Section -->
      <div id="features" class="grid md:grid-cols-3 gap-8 mb-16">
        <div class="bg-white p-6 rounded-xl shadow-md hover:shadow-lg transition">
          <div class="w-12 h-12 bg-indigo-100 rounded-lg flex items-center justify-center mb-4">
            <svg class="w-6 h-6 text-indigo-600" fill="none" stroke="currentColor" viewBox="0 0 24 24">
              <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M12 15v2m-6 4h12a2 2 0 002-2v-6a2 2 0 00-2-2H6a2 2 0 00-2 2v6a2 2 0 002 2zm10-10V7a4 4 0 00-8 0v4h8z"></path>
            </svg>
          </div>
          <h3 class="text-xl font-semibold text-gray-900 mb-2">Local-First Privacy</h3>
          <p class="text-gray-600">
            All your data stays in your browser's IndexedDB. No servers required, complete privacy by default.
          </p>
        </div>

        <div class="bg-white p-6 rounded-xl shadow-md hover:shadow-lg transition">
          <div class="w-12 h-12 bg-green-100 rounded-lg flex items-center justify-center mb-4">
            <svg class="w-6 h-6 text-green-600" fill="none" stroke="currentColor" viewBox="0 0 24 24">
              <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M9 12l2 2 4-4m5.618-4.016A11.955 11.955 0 0112 2.944a11.955 11.955 0 01-8.618 3.04A12.02 12.02 0 003 9c0 5.591 3.824 10.29 9 11.622 5.176-1.332 9-6.03 9-11.622 0-1.042-.133-2.052-.382-3.016z"></path>
            </svg>
          </div>
          <h3 class="text-xl font-semibold text-gray-900 mb-2">Cryptographically Signed</h3>
          <p class="text-gray-600">
            Every commit is signed with your keypair stored in WebCrypto. Tamper-proof and verifiable.
          </p>
        </div>

        <div class="bg-white p-6 rounded-xl shadow-md hover:shadow-lg transition">
          <div class="w-12 h-12 bg-purple-100 rounded-lg flex items-center justify-center mb-4">
            <svg class="w-6 h-6 text-purple-600" fill="none" stroke="currentColor" viewBox="0 0 24 24">
              <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M7 16a4 4 0 01-.88-7.903A5 5 0 1115.9 6L16 6a5 5 0 011 9.9M15 13l-3-3m0 0l-3 3m3-3v12"></path>
            </svg>
          </div>
          <h3 class="text-xl font-semibold text-gray-900 mb-2">Optional Publishing</h3>
          <p class="text-gray-600">
            Publish to external PDS servers when you want. Your choice, your control, your timeline.
          </p>
        </div>

        <div class="bg-white p-6 rounded-xl shadow-md hover:shadow-lg transition">
          <div class="w-12 h-12 bg-blue-100 rounded-lg flex items-center justify-center mb-4">
            <svg class="w-6 h-6 text-blue-600" fill="none" stroke="currentColor" viewBox="0 0 24 24">
              <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M4 7v10c0 2.21 3.582 4 8 4s8-1.79 8-4V7M4 7c0 2.21 3.582 4 8 4s8-1.79 8-4M4 7c0-2.21 3.582-4 8-4s8 1.79 8 4"></path>
            </svg>
          </div>
          <h3 class="text-xl font-semibold text-gray-900 mb-2">ATProto Compatible</h3>
          <p class="text-gray-600">
            Fully compatible with the AT Protocol. Works with the broader ecosystem of decentralized apps.
          </p>
        </div>

        <div class="bg-white p-6 rounded-xl shadow-md hover:shadow-lg transition">
          <div class="w-12 h-12 bg-yellow-100 rounded-lg flex items-center justify-center mb-4">
            <svg class="w-6 h-6 text-yellow-600" fill="none" stroke="currentColor" viewBox="0 0 24 24">
              <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M13 10V3L4 14h7v7l9-11h-7z"></path>
            </svg>
          </div>
          <h3 class="text-xl font-semibold text-gray-900 mb-2">Rust + WebAssembly</h3>
          <p class="text-gray-600">
            Built with Rust for performance and safety. Compiled to WebAssembly for native-speed execution.
          </p>
        </div>

        <div class="bg-white p-6 rounded-xl shadow-md hover:shadow-lg transition">
          <div class="w-12 h-12 bg-red-100 rounded-lg flex items-center justify-center mb-4">
            <svg class="w-6 h-6 text-red-600" fill="none" stroke="currentColor" viewBox="0 0 24 24">
              <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M4 4v5h.582m15.356 2A8.001 8.001 0 004.582 9m0 0H9m11 11v-5h-.581m0 0a8.003 8.003 0 01-15.357-2m15.357 2H15"></path>
            </svg>
          </div>
          <h3 class="text-xl font-semibold text-gray-900 mb-2">Conflict Resolution</h3>
          <p class="text-gray-600">
            Uses Automerge for deterministic, conflict-free merging of mutable records. No data loss.
          </p>
        </div>
      </div>

      <!-- About Section -->
      <div id="about" class="bg-white rounded-xl shadow-md p-8">
        <h2 class="text-3xl font-bold text-gray-900 mb-4">About Browser PDS</h2>
        <div class="prose prose-lg max-w-none text-gray-600">
          <p class="mb-4">
            Browser PDS is a revolutionary approach to personal data storage. Instead of relying on centralized servers,
            your data lives entirely in your browser using IndexedDB for persistence. This means:
          </p>
          <ul class="list-disc list-inside space-y-2 mb-4">
            <li>Complete data ownership - you control everything</li>
            <li>No server costs or maintenance</li>
            <li>Works offline by default</li>
            <li>Privacy-first architecture</li>
            <li>Export and backup anytime</li>
          </ul>
          <p>
            When you're ready to share your data with the world, you can selectively publish records to external
            AT Protocol servers. But that's entirely optional - your PDS works perfectly fine as a local-only solution.
          </p>
        </div>
      </div>
    </main>

    <!-- Footer -->
    <footer class="bg-white mt-16 border-t border-gray-200">
      <div class="max-w-7xl mx-auto px-4 sm:px-6 lg:px-8 py-8">
        <div class="text-center text-gray-600">
          <p>Built with Rust, WebAssembly, and ❤️</p>
          <p class="mt-2 text-sm">Open source and privacy-focused</p>
        </div>
      </div>
    </footer>
  </div>
`
