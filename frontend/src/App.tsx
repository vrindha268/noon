import React, { useState, lazy, Suspense } from 'react';
import { BrowserRouter as Router, Routes, Route, Link, Navigate, useNavigate } from 'react-router-dom';
import { LogOut, Loader2, Lock, MessageSquare, Cpu, ShieldCheck, BarChart3, Key, ChevronDown, ChevronUp, Globe } from 'lucide-react';
import { SunLogo } from './components/logo';
import { UnifiedAuthProvider, useUnifiedAuth } from './auth';
import { ThemeProvider } from './ThemeContext';
import { ThemeToggle } from './components/ThemeToggle';
import './App.css';

const FormBuilder = lazy(() => import('./components/FormBuilder').then(m => ({ default: m.FormBuilder })));
const FormSubmission = lazy(() => import('./components/FormSubmission').then(m => ({ default: m.FormSubmission })));
const FormResults = lazy(() => import('./components/FormResults').then(m => ({ default: m.FormResults })));
const Dashboard = lazy(() => import('./components/Dashboard').then(m => ({ default: m.Dashboard })));
const Subscription = lazy(() => import('./components/Subscription').then(m => ({ default: m.Subscription })));

const PageLoader = () => (
  <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'center', minHeight: '40vh' }}>
    <Loader2 className="spinner" size={32} style={{ color: 'var(--accent)', animation: 'spin 2s linear infinite' }} />
  </div>
);

function Home() {
  const [targetId, setTargetId] = useState('');
  const navigate = useNavigate();
  const { isAuthenticated } = useUnifiedAuth();

  const handleJoin = (e: React.FormEvent) => {
    e.preventDefault();
    if (targetId.trim()) {
      let id = targetId.trim();
      // Handle full URLs if pasted
      if (id.includes('/forms/')) {
        id = id.split('/forms/').pop()?.split('?')[0] || '';
      }
      if (id) {
        navigate(`/forms/${id}`);
      }
    }
  };

  return (
    <div className="home animate-fade-in">
      <div className="hero">
        <div className="noon-logo noon-logo-hero" style={{ marginBottom: '2rem' }}>
          <span>N</span>
          <SunLogo height={160} />
          <span>N</span>
        </div>
        <h1>TRULY ANONYMOUS<br />SURVEYS</h1>

        <div className="home-search-container">
          <form onSubmit={handleJoin} className="search-form">
            <input
              type="text"
              placeholder="Paste form URL or ID here..."
              value={targetId}
              onChange={(e) => setTargetId(e.target.value)}
              className="search-input"
            />
            <button type="submit" className="primary-button join-button">
              Fill Form
            </button>
          </form>

          <div className="search-divider">
            <span>OR</span>
          </div>

          <div className="home-actions">
            <Link to="/create" className="secondary-button large">
              Create New Form
            </Link>
            {isAuthenticated && (
              <div style={{ marginTop: '1rem', display: 'flex', flexDirection: 'column', gap: '0.5rem' }}>
                <Link to="/dashboard" className="text-button">
                  Go to Dashboard
                </Link>
                <Link to="/subscription" className="text-button" style={{ color: 'var(--accent)' }}>
                  Upgrade Subscription
                </Link>
              </div>
            )}
          </div>
        </div>
      </div>
    </div>
  );
}

function About() {
  const [openFaq, setOpenFaq] = useState<number | null>(null);

  const toggleFaq = (index: number) => {
    setOpenFaq(openFaq === index ? null : index);
  };

  const faqs = [
    {
      q: "What makes Noon truly anonymous compared to Google Forms or Typeform?",
      a: "Traditional tools rely on privacy policies and still record your IP address, browser fingerprint, or account token. Noon uses RSA blind signatures—a cryptographic protocol that guarantees your identity is separated from your response before it reaches our servers, making tracking mathematically impossible."
    },
    {
      q: "How do RSA blind signatures work in anonymous feedback forms?",
      a: "When you submit a form, your browser blinds (encrypts) the response before sending it to the Noon server to be cryptographically signed. The server verifies your permission to submit without seeing the response itself. Your browser then unblinds the signature, resulting in a verified, completely anonymous submission."
    },
    {
      q: "Can Noon admins or developers trace who submitted a response?",
      a: "No. Due to the mathematical design of RSA blind signatures, it is physically and cryptographically impossible for admins, developers, or even Noon's own servers to link a form response to the IP address or user who sent it."
    },
    {
      q: "Do users need a cryptographic key or technical knowledge to submit feedback?",
      a: "Not at all. All the complex cryptographic blinding, signing, and verification happen completely in the background in the user's browser. To a user, submitting a Noon form is as simple and fast as filling out any standard online form."
    },
    {
      q: "Is Noon free to use, and is it open source?",
      a: "Yes, Noon is fully open source under the MIT license, allowing anyone to inspect the cryptographic protocol. The platform offers a generous free tier for secure, anonymous feedback, with premium subscription plans available for advanced form building and team management."
    }
  ];

  return (
    <div className="about-page animate-fade-in" style={{ padding: '2rem 0' }}>
      {/* Cryptographic Differentiator Block (Matching Reference Image) */}
      <div className="seo-intro-section" style={{ marginTop: 0 }}>
        <div className="intro-container">
          <div className="intro-meta">
            <span className="intro-tagline-main">PRODUCT</span>
            <h2 className="intro-title">Noon</h2>
            <p className="intro-subtitle">Anonymous feedback forms — impossible to trace, by design.</p>
          </div>

          <div className="intro-card-description">
            <p>
              Noon lets people share honest feedback without fear. Unlike ordinary anonymous forms that just promise not to track you, Noon uses <strong>RSA blind signatures</strong> — a cryptographic method that makes it mathematically impossible to link a submission back to the person who sent it. Not even us.
            </p>
          </div>

          <div className="badges-row">
            <span className="badge-item badge-cryptographic">
              <ShieldCheck size={14} /> Cryptographic anonymity
            </span>
            <span className="badge-item badge-honest">
              <BarChart3 size={14} /> Honest feedback
            </span>
            <span className="badge-item badge-traceability">
              <Key size={14} /> Zero traceability
            </span>
          </div>
        </div>
      </div>

      {/* WHY IT'S DIFFERENT SECTION (Matching Reference Image) */}
      <div className="different-section">
        <h3 className="section-title-uppercase">WHY IT'S DIFFERENT</h3>
        
        <div className="different-grid">
          <div className="different-card">
            <div className="different-icon-wrapper lock-icon-wrapper">
              <Lock size={20} />
            </div>
            <div className="different-content">
              <h4>Privacy by math, not policy</h4>
              <p>Other tools say "we won't track you." Noon says "we can't." RSA blind signatures ensure the server never sees the identity behind a response — even during verification.</p>
            </div>
          </div>

          <div className="different-card">
            <div className="different-icon-wrapper message-icon-wrapper">
              <MessageSquare size={20} />
            </div>
            <div className="different-content">
              <h4>Real honesty, finally</h4>
              <p>When people know they truly can't be identified, they speak freely — giving organizations the unfiltered truth they need to actually improve.</p>
            </div>
          </div>

          <div className="different-card">
            <div className="different-icon-wrapper complex-icon-wrapper">
              <Cpu size={20} />
            </div>
            <div className="different-content">
              <h4>Simple to use, complex under the hood</h4>
              <p>Submitters fill out a form. Admins get responses. The cryptography happens invisibly in between — no technical knowledge needed on either side.</p>
            </div>
          </div>
        </div>
      </div>

      {/* FAQ SECTION */}
      <div className="faq-section-landing">
        <h3 className="section-title-uppercase">FREQUENTLY ASKED QUESTIONS</h3>
        <div className="faq-list">
          {faqs.map((faq, index) => (
            <div key={index} className={`faq-item-card ${openFaq === index ? 'active' : ''}`} onClick={() => toggleFaq(index)}>
              <div className="faq-question-header">
                <h5>{faq.q}</h5>
                <span className="faq-toggle-icon">
                  {openFaq === index ? <ChevronUp size={18} /> : <ChevronDown size={18} />}
                </span>
              </div>
              {openFaq === index && (
                <div className="faq-answer-body">
                  <p>{faq.a}</p>
                </div>
              )}
            </div>
          ))}
        </div>
      </div>

      {/* SEO STRATEGY SECTION (Matching Reference Image) */}
      <div className="seo-strategy-section">
        <h3 className="section-title-uppercase">SEO STRATEGY</h3>
        <div className="seo-strategy-card">
          <div className="seo-strategy-header">
            <div className="seo-strategy-icon">
              <Globe size={20} />
            </div>
            <h4>Target keywords</h4>
          </div>

          <div className="keywords-grid">
            <div className="keyword-item-card">
              <div className="keyword-name">anonymous feedback forms</div>
              <div className="keyword-meta">High Intent, core use case</div>
            </div>
            <div className="keyword-item-card">
              <div className="keyword-name">untraceable employee surveys</div>
              <div className="keyword-meta">B2B, HR teams</div>
            </div>
            <div className="keyword-item-card">
              <div className="keyword-name">truly anonymous survey tool</div>
              <div className="keyword-meta">Differentiator keyword</div>
            </div>
            <div className="keyword-item-card">
              <div className="keyword-name">blind signature feedback app</div>
              <div className="keyword-meta">Technical audience</div>
            </div>
            <div className="keyword-item-card">
              <div className="keyword-name">honest workplace feedback</div>
              <div className="keyword-meta">High volume, culture-driven</div>
            </div>
            <div className="keyword-item-card">
              <div className="keyword-name">secure anonymous forms</div>
              <div className="keyword-meta">Privacy-conscious users</div>
            </div>
          </div>
        </div>
      </div>
    </div>
  );
}

function NavBarContent() {
  const { isAuthenticated, isInitialLoading, email, logout } = useUnifiedAuth();

  return (
    <nav className="navbar-nav">
      <Link to="/about">About</Link>
      <Link to="/dashboard">Dashboard</Link>
      <Link to="/subscription">Pricing</Link>
      <a href="https://github.com/lupyd/noon.git" target="_blank" rel="noopener noreferrer" style={{ display: 'flex', alignItems: 'center' }} title="View on GitHub">
        <svg height="20" viewBox="0 0 16 16" width="20" fill="currentColor"><path d="M8 0C3.58 0 0 3.58 0 8c0 3.54 2.29 6.53 5.47 7.59.4.07.55-.17.55-.38 0-.19-.01-.82-.01-1.49-2.01.37-2.53-.49-2.69-.94-.09-.23-.48-.94-.82-1.13-.28-.15-.68-.52-.01-.53.63-.01 1.08.58 1.23.82.72 1.21 1.87.87 2.33.66.07-.52.28-.87.51-1.07-1.78-.2-3.64-.89-3.64-3.95 0-.87.31-1.59.82-2.15-.08-.2-.36-1.02.08-2.12 0 0 .67-.21 2.2.82.64-.18 1.32-.27 2-.27.68 0 1.36.09 2 .27 1.53-1.04 2.2-.82 2.2-.82.44 1.1.16 1.92.08 2.12.51.56.82 1.27.82 2.15 0 3.07-1.87 3.75-3.65 3.95.29.25.54.73.54 1.48 0 1.07-.01 1.93-.01 2.2 0 .21.15.46.55.38A8.013 8.013 0 0016 8c0-4.42-3.58-8-8-8z"></path></svg>
      </a>
      <ThemeToggle />

      <div className="auth-section" style={{ borderLeft: isAuthenticated ? '1px solid var(--border)' : 'none' }}>
        {isInitialLoading ? (
          <span className="text-muted">Loading...</span>
        ) : isAuthenticated && (
          <>
            <span className="user-greeting" style={{ maxWidth: '150px', overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>
              Hi, {email?.split('@')[0]}
            </span>
            <button
              onClick={() => logout()}
              className="icon-button"
              title="Log Out"
            >
              <LogOut size={18} />
            </button>
          </>
        )}
      </div>
    </nav>
  );
}

function App() {
  return (
    <Router>
      <ThemeProvider>
        <UnifiedAuthProvider>
          <div className="App">
            <header className="navbar">
              <div className="container">
                <Link to="/" className="logo">
                  <div className="noon-logo">
                    <span>N</span>
                    <SunLogo height={40} />
                    <span>N</span>
                  </div>
                </Link>
                <NavBarContent />
              </div>
            </header>

            <main className="container main-content">
              <Suspense fallback={<PageLoader />}>
                <Routes>
                  <Route path="/" element={<Home />} />
                  <Route path="/index.html" element={<Navigate to="/" replace />} />
                  <Route path="/about" element={<About />} />
                  <Route path="/dashboard" element={<Dashboard />} />
                  <Route path="/create" element={<FormBuilder />} />
                  <Route path="/forms/:id" element={<FormSubmission />} />
                  <Route path="/forms/:id/results" element={<FormResults />} />
                  <Route path="/subscription" element={<Subscription />} />
                </Routes>
              </Suspense>
            </main>

            <footer className="footer">
              <div className="container">
                <p>&copy; 2026 Lupyd Foundation. All rights reserved.</p>
              </div>
            </footer>
          </div>
        </UnifiedAuthProvider>
      </ThemeProvider>
    </Router>
  );
}

export default App;
