document.addEventListener('DOMContentLoaded', () => {
    // Staggered letter reveal on hero title
    const heroTitle = document.querySelector('.hero-title');
    if (heroTitle) {
        let charIndex = 0;
        const fragment = document.createDocumentFragment();

        heroTitle.childNodes.forEach(node => {
            if (node.nodeType === Node.ELEMENT_NODE) {
                // Preserve existing elements (like the gradient span)
                const clone = node.cloneNode(false);
                const text = node.textContent || '';
                for (const ch of text) {
                    if (ch === ' ') {
                        clone.appendChild(document.createTextNode(' '));
                    } else {
                        const span = document.createElement('span');
                        span.className = 'char';
                        span.style.animationDelay = `${charIndex * 0.03}s`;
                        span.textContent = ch;
                        clone.appendChild(span);
                        charIndex++;
                    }
                }
                fragment.appendChild(clone);
            } else if (node.nodeType === Node.TEXT_NODE) {
                const text = node.textContent || '';
                for (const ch of text) {
                    if (ch === ' ') {
                        fragment.appendChild(document.createTextNode(' '));
                    } else {
                        const span = document.createElement('span');
                        span.className = 'char';
                        span.style.animationDelay = `${charIndex * 0.03}s`;
                        span.textContent = ch;
                        fragment.appendChild(span);
                        charIndex++;
                    }
                }
            }
        });

        heroTitle.replaceChildren(fragment);
    }

    // Navbar scroll effect with logo glow
    const navbar = document.querySelector('.navbar');

    window.addEventListener('scroll', () => {
        if (window.scrollY > 50) {
            navbar.classList.add('scrolled');
        } else {
            navbar.classList.remove('scrolled');
        }
    }, { passive: true });

    // Intersection Observer for scroll animations
    const observerOptions = {
        root: null,
        rootMargin: '0px',
        threshold: 0.1
    };

    const observer = new IntersectionObserver((entries) => {
        entries.forEach(entry => {
            if (entry.isIntersecting) {
                entry.target.classList.add('visible');
            }
        });
    }, observerOptions);

    const scrollTriggers = document.querySelectorAll('.scroll-trigger');
    scrollTriggers.forEach(el => observer.observe(el));

    // Smoother parallax effect for glowing orbs (reduced intensity)
    const orbs = document.querySelectorAll('.glow-orb');
    let mouseX = 0.5;
    let mouseY = 0.5;
    let currentX = 0.5;
    let currentY = 0.5;

    window.addEventListener('mousemove', (e) => {
        mouseX = e.clientX / window.innerWidth;
        mouseY = e.clientY / window.innerHeight;
    }, { passive: true });

    function animateOrbs() {
        // Lerp for smoothness
        currentX += (mouseX - currentX) * 0.02;
        currentY += (mouseY - currentY) * 0.02;

        orbs.forEach((orb, index) => {
            const factor = index === 0 ? 15 : -20;
            orb.style.transform = `translate(${currentX * factor}px, ${currentY * factor}px)`;
        });

        requestAnimationFrame(animateOrbs);
    }

    animateOrbs();
});
