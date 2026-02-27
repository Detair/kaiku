import { Component } from "solid-js";
import { themeState, setTheme } from "@/stores/theme";
import CodeBlock from "@/components/ui/CodeBlock";

const ThemeDemo: Component = () => {
  const exampleCode = {
    rust: `fn main() {
    println!("Hello, world!");
    let numbers = vec![1, 2, 3, 4, 5];
    for num in numbers.iter() {
        println!("{}", num);
    }
}`,
    typescript: `interface User {
  id: string;
  name: string;
  email: string;
}

async function fetchUser(id: string): Promise<User> {
  const response = await fetch(\`/api/users/\${id}\`);
  return response.json();
}`,
    python: `def calculate_fibonacci(n: int) -> list[int]:
    """Calculate Fibonacci sequence up to n terms."""
    fib = [0, 1]
    for i in range(2, n):
        fib.append(fib[i-1] + fib[i-2])
    return fib

print(calculate_fibonacci(10))`,
  };

  return (
    <div class="min-h-screen bg-surface-base p-8">
      <div class="max-w-4xl mx-auto space-y-8">
        {/* Theme Selector */}
        <div class="bg-surface-layer1 rounded-2xl p-6 border border-white/10">
          <h1 class="text-3xl font-bold text-text-primary mb-2">
            Theme System Demo
          </h1>
          <p class="text-text-secondary mb-6">
            Select a theme to see colors update instantly
          </p>

          <div class="space-y-3">
            {themeState.availableThemes.map((theme) => (
              <button
                onClick={() => setTheme(theme.id)}
                class="w-full text-left p-4 rounded-xl border-2 transition-all"
                classList={{
                  "border-accent-primary bg-accent-primary/10":
                    themeState.currentTheme === theme.id,
                  "border-white/10 hover:border-accent-primary/50 bg-surface-layer2":
                    themeState.currentTheme !== theme.id,
                }}
              >
                <div class="flex items-start gap-3">
                  <div
                    class="w-5 h-5 rounded-full border-2 flex-shrink-0 mt-0.5"
                    classList={{
                      "border-accent-primary bg-accent-primary":
                        themeState.currentTheme === theme.id,
                      "border-white/30": themeState.currentTheme !== theme.id,
                    }}
                  >
                    {themeState.currentTheme === theme.id && (
                      <div class="w-2 h-2 bg-surface-base rounded-full m-0.5" />
                    )}
                  </div>
                  <div class="flex-1">
                    <div class="font-semibold text-text-primary">
                      {theme.name}
                    </div>
                    <div class="text-sm text-text-secondary">
                      {theme.description}
                    </div>
                    <div class="flex gap-2 mt-2">
                      <div class="w-6 h-6 rounded bg-surface-base border border-white/10" />
                      <div class="w-6 h-6 rounded bg-surface-layer1 border border-white/10" />
                      <div class="w-6 h-6 rounded bg-surface-layer2 border border-white/10" />
                      <div class="w-6 h-6 rounded bg-accent-primary border border-white/10" />
                    </div>
                  </div>
                </div>
              </button>
            ))}
          </div>
        </div>

        {/* Code Highlighting Demo */}
        <div class="bg-surface-layer1 rounded-2xl p-6 border border-white/10">
          <h2 class="text-2xl font-bold text-text-primary mb-2">
            Code Syntax Highlighting
          </h2>
          <p class="text-text-secondary mb-6">
            Code blocks with theme-aware syntax colors
          </p>

          <div class="space-y-4">
            <div>
              <h3 class="text-lg font-semibold text-text-primary mb-2">Rust</h3>
              <CodeBlock language="rust">{exampleCode.rust}</CodeBlock>
            </div>

            <div>
              <h3 class="text-lg font-semibold text-text-primary mb-2">
                TypeScript
              </h3>
              <CodeBlock language="typescript">
                {exampleCode.typescript}
              </CodeBlock>
            </div>

            <div>
              <h3 class="text-lg font-semibold text-text-primary mb-2">
                Python
              </h3>
              <CodeBlock language="python">{exampleCode.python}</CodeBlock>
            </div>
          </div>
        </div>

        {/* Color Reference */}
        <div class="bg-surface-layer1 rounded-2xl p-6 border border-white/10">
          <h2 class="text-2xl font-bold text-text-primary mb-4">
            Theme Colors
          </h2>
          <div class="grid grid-cols-2 gap-4">
            <div class="space-y-2">
              <div class="flex items-center gap-3">
                <div class="w-12 h-12 rounded-lg bg-surface-base border border-white/10" />
                <div>
                  <div class="text-sm font-medium text-text-primary">
                    Surface Base
                  </div>
                  <div class="text-xs text-text-secondary">Background</div>
                </div>
              </div>
              <div class="flex items-center gap-3">
                <div class="w-12 h-12 rounded-lg bg-surface-layer1 border border-white/10" />
                <div>
                  <div class="text-sm font-medium text-text-primary">
                    Surface Layer 1
                  </div>
                  <div class="text-xs text-text-secondary">Panels</div>
                </div>
              </div>
              <div class="flex items-center gap-3">
                <div class="w-12 h-12 rounded-lg bg-surface-layer2 border border-white/10" />
                <div>
                  <div class="text-sm font-medium text-text-primary">
                    Surface Layer 2
                  </div>
                  <div class="text-xs text-text-secondary">Code blocks</div>
                </div>
              </div>
            </div>
            <div class="space-y-2">
              <div class="flex items-center gap-3">
                <div class="w-12 h-12 rounded-lg bg-accent-primary border border-white/10" />
                <div>
                  <div class="text-sm font-medium text-text-primary">
                    Accent Primary
                  </div>
                  <div class="text-xs text-text-secondary">
                    Interactive elements
                  </div>
                </div>
              </div>
              <div class="flex items-center gap-3">
                <div class="w-12 h-12 rounded-lg bg-accent-danger border border-white/10" />
                <div>
                  <div class="text-sm font-medium text-text-primary">
                    Accent Danger
                  </div>
                  <div class="text-xs text-text-secondary">
                    Destructive actions
                  </div>
                </div>
              </div>
              <div class="flex items-center gap-3">
                <div class="w-12 h-12 rounded-lg flex items-center justify-center border border-white/10 text-text-primary font-medium">
                  Aa
                </div>
                <div>
                  <div class="text-sm font-medium text-text-primary">
                    Text Primary
                  </div>
                  <div class="text-xs text-text-secondary">Main content</div>
                </div>
              </div>
            </div>
          </div>
        </div>
      </div>
    </div>
  );
};

export default ThemeDemo;
