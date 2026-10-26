package io.wolftown.kaiku.lint

import com.android.tools.lint.checks.infrastructure.TestFile
import com.android.tools.lint.checks.infrastructure.TestFiles.java
import com.android.tools.lint.checks.infrastructure.TestFiles.kotlin
import com.android.tools.lint.checks.infrastructure.TestLintResult
import com.android.tools.lint.checks.infrastructure.TestLintTask.lint
import org.junit.Test

class ComposeHardcodedTextDetectorTest {

    private val composableStub: TestFile = kotlin(
        """
        package androidx.compose.runtime

        @Retention(AnnotationRetention.BINARY)
        @Target(
            AnnotationTarget.FUNCTION,
            AnnotationTarget.TYPE,
            AnnotationTarget.TYPE_PARAMETER,
            AnnotationTarget.PROPERTY_GETTER,
        )
        annotation class Composable
        """
    ).indented()

    private val material3Stub: TestFile = kotlin(
        """
        package androidx.compose.material3

        import androidx.compose.runtime.Composable

        @Composable
        fun Text(text: String) {}

        @Composable
        fun Icon(imageVector: Any? = null, contentDescription: String?) {}
        """
    ).indented()

    private val stringResourceStub: TestFile = kotlin(
        """
        package androidx.compose.ui.res

        import androidx.compose.runtime.Composable

        @Composable
        fun stringResource(id: Int): String = id.toString()
        """
    ).indented()

    private val notificationStub: TestFile = kotlin(
        """
        package androidx.core.app

        class NotificationCompat {
            class Builder {
                fun setContentTitle(title: CharSequence?): Builder = this
            }
        }
        """
    ).indented()

    private val animationStub: TestFile = kotlin(
        """
        package androidx.compose.animation.core

        import androidx.compose.runtime.Composable

        @Composable
        fun animateFloatAsState(targetValue: Float, label: String): Float = targetValue
        """
    ).indented()

    private val contextStub: TestFile = java(
        """
        package android.content;

        public abstract class Context {}
        """
    ).indented()

    private val notificationChannelStub: TestFile = java(
        """
        package android.app;

        public final class NotificationChannel {
            public NotificationChannel(String id, CharSequence name, int importance) {}
        }
        """
    ).indented()

    private val toastStub: TestFile = java(
        """
        package android.widget;

        import android.content.Context;

        public class Toast {
            public static Toast makeText(Context context, CharSequence text, int duration) {
                return new Toast();
            }
        }
        """
    ).indented()

    private val rClass: TestFile = java(
        """
        package test.pkg;

        public final class R {
            public static final class string {
                public static final int x = 0x7f010000;
            }
        }
        """
    ).indented()

    private val stubs = arrayOf(
        composableStub,
        material3Stub,
        stringResourceStub,
        notificationStub,
        animationStub,
        contextStub,
        notificationChannelStub,
        toastStub,
        rClass,
    )

    private fun check(source: String): TestLintResult =
        lint()
            .files(kotlin(source).indented(), *stubs)
            .issues(ComposeHardcodedTextDetector.ISSUE)
            .allowMissingSdk()
            .run()

    @Test
    fun `flags Text literal`() {
        check(
            """
            package test.pkg

            import androidx.compose.material3.Text
            import androidx.compose.runtime.Composable

            @Composable
            fun Screen() {
                Text("Settings")
            }
            """
        ).expectErrorCount(1)
    }

    @Test
    fun `flags contentDescription literal`() {
        check(
            """
            package test.pkg

            import androidx.compose.material3.Icon
            import androidx.compose.runtime.Composable

            @Composable
            fun Screen() {
                Icon(contentDescription = "Back")
            }
            """
        ).expectErrorCount(1)
    }

    @Test
    fun `flags project composable label`() {
        check(
            """
            package test.pkg

            import androidx.compose.runtime.Composable

            @Composable
            fun Row(label: String) {}

            @Composable
            fun Screen() {
                Row(label = "Username")
            }
            """
        ).expectErrorCount(1)
    }

    @Test
    fun `flags template with words`() {
        check(
            """
            package test.pkg

            import androidx.compose.material3.Text
            import androidx.compose.runtime.Composable

            @Composable
            fun Screen(name: String) {
                Text("Hello ${'$'}name")
            }
            """
        ).expectErrorCount(1)
    }

    @Test
    fun `flags notification title literal`() {
        check(
            """
            package test.pkg

            import androidx.core.app.NotificationCompat

            fun build(builder: NotificationCompat.Builder) {
                builder.setContentTitle("Voice call")
            }
            """
        ).expectErrorCount(1)
    }

    @Test
    fun `ignores stringResource and symbols`() {
        check(
            """
            package test.pkg

            import androidx.compose.material3.Text
            import androidx.compose.runtime.Composable
            import androidx.compose.ui.res.stringResource

            @Composable
            fun Screen() {
                Text(stringResource(R.string.x))
                Text("99+")
                Text("#")
            }
            """
        ).expectClean()
    }

    @Test
    fun `flags notification channel name literal`() {
        check(
            """
            package test.pkg

            import android.app.NotificationChannel

            fun channel(): NotificationChannel = NotificationChannel("voice_calls", "Voice calls", 2)
            """
        ).expectErrorCount(1)
    }

    @Test
    fun `flags toast text literal`() {
        check(
            """
            package test.pkg

            import android.content.Context
            import android.widget.Toast

            fun show(ctx: Context) {
                Toast.makeText(ctx, "Copied", 0)
            }
            """
        ).expectErrorCount(1)
    }

    @Test
    fun `ignores animation labels`() {
        check(
            """
            package test.pkg

            import androidx.compose.animation.core.animateFloatAsState
            import androidx.compose.runtime.Composable

            @Composable
            fun Screen(): Float = animateFloatAsState(1f, label = "fade")
            """
        ).expectClean()
    }

    @Test
    fun `ignores format specifiers and letterless templates`() {
        check(
            """
            package test.pkg

            import androidx.compose.material3.Text
            import androidx.compose.runtime.Composable

            @Composable
            fun Screen(a: Int, b: Int) {
                Text("/%1\${'$'}d")
                Text("${'$'}a/${'$'}b")
            }
            """
        ).expectClean()
    }

    @Test
    fun `ignores text parameter of non-composable function`() {
        check(
            """
            package test.pkg

            fun log(text: String) {}

            fun run() {
                log(text = "Hello")
            }
            """
        ).expectClean()
    }
}
