import React, { useState, useEffect } from 'react';
import { useUnifiedAuth } from '../auth';
import { API_URL } from '../config';
import { Loader2, CheckCircle, CreditCard, ShieldCheck, Zap } from 'lucide-react';

interface SubscriptionStatus {
    tier: string;
    subscription_status: string;
    razorpay_subscription_id: string | null;
    current_period_end: number | null;
    max_participants: number;
}

interface SubscriptionConfig {
    free_max_participants: number;
    pro_max_participants: number;
    team_max_participants: number;
}




function loadScript(src: string) {
  const id = "razorpay-checkout-script"
  if (document.getElementById(id)) {
    return
  }
  return new Promise((resolve) => {
    const script = document.createElement("script");
    script.id = id
    script.src = src;
    script.onload = () => {
      resolve(true);
    };
    script.onerror = () => {
      resolve(false);
    };
    document.body.appendChild(script);
  });
}

async function loadRazorpay() {
  const res = await loadScript(
    "https://checkout.razorpay.com/v1/checkout.js"
  );

  if (res) {
    console.log(`Razorpay SDK loaded`)
  } else {
    console.error(`Razorpay SDK failed to load`)
  }
}


export const Subscription: React.FC = () => {
    const { isAuthenticated, getAuthHeaders, email } = useUnifiedAuth();
    const [status, setStatus] = useState<SubscriptionStatus | null>(null);
    const [config, setConfig] = useState<SubscriptionConfig | null>(null);
    const [loading, setLoading] = useState(true);
    const [submitting, setSubmitting] = useState<string | null>(null);
    const [error, setError] = useState<string | null>(null);

    const fetchStatusAndConfig = async () => {
        try {
            const configResp = await fetch(`${API_URL}/subscription/config`);
            if (configResp.ok) {
                const configData = await configResp.json();
                setConfig(configData);
            }

            if (isAuthenticated) {
                const headers = await getAuthHeaders();
                const resp = await fetch(`${API_URL}/subscription/status`, { headers });
                if (resp.ok) {
                    const data = await resp.json();
                    setStatus(data);
                }
            }
        } catch (e) {
            console.error("Failed to fetch subscription status or config", e);
        } finally {
            setLoading(false);
        }
    };

    useEffect(() => {
        fetchStatusAndConfig();
    }, [isAuthenticated]);

    const handleSubscribe = async (tier: 'pro' | 'team') => {
        setSubmitting(tier);
        setError(null);
        try {
            const rzpPromise = loadRazorpay()
            const headers = await getAuthHeaders();
            const resp = await fetch(`${API_URL}/subscription/create`, {
                method: 'POST',
                headers: {
                    ...headers,
                    'Content-Type': 'application/json'
                },
                body: JSON.stringify({ tier })
            });

            if (!resp.ok) {
                const text = await resp.text();
                throw new Error(text || "Failed to create subscription");
            }

            const { subscription_id, key_id } = await resp.json();
            await rzpPromise
            const options = {
                key: key_id,
                subscription_id: subscription_id,
                name: "Noon Forms",
                description: `Upgrade to ${tier.toUpperCase()} Plan`,
                handler: function (_response: any) {
                    // Payment successful
                    // alert("Payment successful! Your subscription will be activated shortly.");
                    fetchStatusAndConfig();
                },
                prefill: {
                    email: email || ""
                },
                theme: {
                    color: "#6366f1"
                }
            };

            // @ts-ignore
            const rzp = new window.Razorpay(options);
            rzp.open();

        } catch (e: any) {
            setError(e.message);
        } finally {
            setSubmitting(null);
        }
    };

    const handleCancel = async () => {
        if (!window.confirm("Are you sure you want to cancel your subscription? Your access to premium features will be removed.")) {
            return;
        }

        setSubmitting('cancel');
        setError(null);
        try {
            const headers = await getAuthHeaders();
            const resp = await fetch(`${API_URL}/subscription/cancel`, {
                method: 'POST',
                headers
            });

            if (!resp.ok) {
                const text = await resp.text();
                throw new Error(text || "Failed to cancel subscription");
            }

            alert("Subscription cancellation requested. It may take a few moments to update.");
            fetchStatusAndConfig();
        } catch (e: any) {
            setError(e.message);
        } finally {
            setSubmitting(null);
        }
    };

    if (!isAuthenticated) {
        return (
            <div className="container animate-fade-in" style={{ textAlign: 'center', padding: '4rem 1rem' }}>
                <h2>Please log in to manage your subscription</h2>
            </div>
        );
    }

    if (loading) {
        return (
            <div style={{ display: 'flex', justifyContent: 'center', padding: '4rem' }}>
                <Loader2 className="spinner" size={48} />
            </div>
        );
    }

    return (
        <div className="container animate-fade-in" style={{ maxWidth: '900px', margin: '0 auto', padding: '2rem 1rem' }}>
            <div style={{ marginBottom: '3rem', textAlign: 'center' }}>
                <h1 style={{ fontSize: '2.5rem', marginBottom: '1rem' }}>Subscription Plans</h1>
                <p className="text-muted" style={{ fontSize: '1.1rem' }}>
                    Current Plan: <span style={{ color: 'var(--accent)', fontWeight: 'bold', textTransform: 'capitalize' }}>{status?.tier || 'free'}</span>
                    {status?.subscription_status === 'active' ? (
                        <span style={{ marginLeft: '10px', fontSize: '0.9rem', padding: '2px 8px', borderRadius: '12px', background: 'rgba(34, 197, 94, 0.2)', color: '#22c55e' }}>
                            Active
                        </span>
                    ) : status?.subscription_status === 'pending' ? (
                        <span style={{ marginLeft: '10px', fontSize: '0.9rem', padding: '2px 8px', borderRadius: '12px', background: 'rgba(234, 179, 8, 0.2)', color: '#eab308' }}>
                            Pending Payment
                        </span>
                    ) : null}
                </p>
                {status?.tier !== 'free' && status?.subscription_status === 'active' && (
                    <button 
                        onClick={handleCancel}
                        disabled={submitting === 'cancel'}
                        className="secondary-button"
                        style={{ marginTop: '0.5rem', color: '#ef4444', borderColor: '#ef4444' }}
                    >
                        {submitting === 'cancel' ? <Loader2 className="spinner" size={16} /> : 'Cancel Subscription'}
                    </button>
                )}
                {status?.current_period_end && (
                    <p style={{ fontSize: '0.9rem' }} className="text-muted">
                        Renews on: {new Date(status.current_period_end * 1000).toLocaleDateString()}
                    </p>
                )}
            </div>

            {error && (
                <div style={{ padding: '1rem', background: 'rgba(239, 68, 68, 0.1)', color: '#ef4444', borderRadius: '8px', marginBottom: '2rem', textAlign: 'center' }}>
                    {error}
                </div>
            )}

            <div style={{ display: 'grid', gridTemplateColumns: 'repeat(auto-fit, minmax(280px, 1fr))', gap: '2rem' }}>
                {/* Free Plan */}
                <div className="card" style={{ padding: '2rem', display: 'flex', flexDirection: 'column', opacity: status?.tier === 'free' ? 0.8 : 1 }}>
                    <div style={{ marginBottom: '1.5rem' }}>
                        <h3 style={{ margin: 0 }}>Free</h3>
                        <div style={{ fontSize: '2rem', fontWeight: 'bold', margin: '0.5rem 0' }}>₹0<span style={{ fontSize: '1rem', fontWeight: 'normal' }}>/mo</span></div>
                    </div>
                    <ul style={{ listStyle: 'none', padding: 0, margin: '0 0 2rem 0', flex: 1 }}>
                        <li style={{ marginBottom: '0.5rem', display: 'flex', alignItems: 'center' }}>
                            <CheckCircle size={16} style={{ marginRight: '8px', color: '#22c55e' }} /> 10 Forms
                        </li>
                        <li style={{ marginBottom: '0.5rem', display: 'flex', alignItems: 'center' }}>
                            <CheckCircle size={16} style={{ marginRight: '8px', color: '#22c55e' }} /> {config ? config.free_max_participants : 10} Participants/Form
                        </li>
                    </ul>
                    <button className="secondary-button" disabled style={{ width: '100%' }}>
                        {status?.tier === 'free' ? 'Current Plan' : 'Free Forever'}
                    </button>
                </div>

                {/* Pro Plan */}
                <div className="card" style={{ padding: '2rem', display: 'flex', flexDirection: 'column', border: status?.tier === 'pro' ? '2px solid var(--accent)' : '1px solid var(--border)', position: 'relative' }}>
                    {status?.tier === 'pro' && (
                        <div style={{ position: 'absolute', top: '-12px', right: '20px', background: 'var(--accent)', color: 'white', padding: '2px 10px', borderRadius: '10px', fontSize: '0.8rem' }}>
                            Current
                        </div>
                    )}
                    <div style={{ marginBottom: '1.5rem' }}>
                        <div style={{ display: 'flex', alignItems: 'center', gap: '8px' }}>
                            <h3 style={{ margin: 0 }}>Pro</h3>
                            <Zap size={18} style={{ color: '#eab308' }} />
                        </div>
                        <div style={{ fontSize: '2rem', fontWeight: 'bold', margin: '0.5rem 0' }}>₹499<span style={{ fontSize: '1rem', fontWeight: 'normal' }}>/mo</span></div>
                    </div>
                    <ul style={{ listStyle: 'none', padding: 0, margin: '0 0 2rem 0', flex: 1 }}>
                        <li style={{ marginBottom: '0.5rem', display: 'flex', alignItems: 'center' }}>
                            <CheckCircle size={16} style={{ marginRight: '8px', color: '#22c55e' }} /> 100 Forms
                        </li>
                        <li style={{ marginBottom: '0.5rem', display: 'flex', alignItems: 'center' }}>
                            <CheckCircle size={16} style={{ marginRight: '8px', color: '#22c55e' }} /> {config ? config.pro_max_participants : 100} Participants/Form
                        </li>
                        <li style={{ marginBottom: '0.5rem', display: 'flex', alignItems: 'center' }}>
                            <CheckCircle size={16} style={{ marginRight: '8px', color: '#22c55e' }} /> Analytics Dashboard
                        </li>
                    </ul>
                    <button 
                        className="primary-button" 
                        disabled={submitting !== null || (status?.tier === 'pro' && status?.subscription_status === 'active')}
                        onClick={() => handleSubscribe('pro')}
                        style={{ width: '100%' }}
                    >
                        {submitting === 'pro' ? <Loader2 className="spinner" size={18} /> : 
                         (status?.tier === 'pro' && status?.subscription_status === 'active' ? 'Active' : 
                          (status?.tier === 'pro' && status?.subscription_status === 'pending' ? 'Complete Payment' : 'Upgrade to Pro'))}
                    </button>
                </div>

                {/* Team Plan */}
                <div className="card" style={{ padding: '2rem', display: 'flex', flexDirection: 'column', border: status?.tier === 'team' ? '2px solid var(--accent)' : '1px solid var(--border)', position: 'relative' }}>
                    {status?.tier === 'team' && (
                        <div style={{ position: 'absolute', top: '-12px', right: '20px', background: 'var(--accent)', color: 'white', padding: '2px 10px', borderRadius: '10px', fontSize: '0.8rem' }}>
                            Current
                        </div>
                    )}
                    <div style={{ marginBottom: '1.5rem' }}>
                        <div style={{ display: 'flex', alignItems: 'center', gap: '8px' }}>
                            <h3 style={{ margin: 0 }}>Team</h3>
                            <ShieldCheck size={18} style={{ color: 'var(--accent)' }} />
                        </div>
                        <div style={{ fontSize: '2rem', fontWeight: 'bold', margin: '0.5rem 0' }}>₹1,999<span style={{ fontSize: '1rem', fontWeight: 'normal' }}>/mo</span></div>
                    </div>
                    <ul style={{ listStyle: 'none', padding: 0, margin: '0 0 2rem 0', flex: 1 }}>
                        <li style={{ marginBottom: '0.5rem', display: 'flex', alignItems: 'center' }}>
                            <CheckCircle size={16} style={{ marginRight: '8px', color: '#22c55e' }} /> 1,000 Forms
                        </li>
                        <li style={{ marginBottom: '0.5rem', display: 'flex', alignItems: 'center' }}>
                            <CheckCircle size={16} style={{ marginRight: '8px', color: '#22c55e' }} /> {config ? config.team_max_participants : 1000} Participants/Form
                        </li>
                        <li style={{ marginBottom: '0.5rem', display: 'flex', alignItems: 'center' }}>
                            <CheckCircle size={16} style={{ marginRight: '8px', color: '#22c55e' }} /> Priority Support
                        </li>
                    </ul>
                    <button 
                        className="primary-button" 
                        disabled={submitting !== null || (status?.tier === 'team' && status?.subscription_status === 'active')}
                        onClick={() => handleSubscribe('team')}
                        style={{ width: '100%', background: 'var(--text)', color: 'var(--bg)' }}
                    >
                        {submitting === 'team' ? <Loader2 className="spinner" size={18} /> : 
                         (status?.tier === 'team' && status?.subscription_status === 'active' ? 'Active' : 
                          (status?.tier === 'team' && status?.subscription_status === 'pending' ? 'Complete Payment' : 'Get Team'))}
                    </button>
                </div>
            </div>

            <div style={{ marginTop: '3rem', textAlign: 'center' }}>
                <p className="text-muted" style={{ fontSize: '0.9rem' }}>
                    Payments processed securely by <span style={{ fontWeight: 'bold' }}>Razorpay</span>.
                </p>
                <div style={{ display: 'flex', justifyContent: 'center', gap: '1rem', marginTop: '1rem', opacity: 0.5 }}>
                    <CreditCard size={24} />
                    {/* Add more icons if needed */}
                </div>
            </div>
        </div>
    );
};
