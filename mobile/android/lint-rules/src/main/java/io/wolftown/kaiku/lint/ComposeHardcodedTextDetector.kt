package io.wolftown.kaiku.lint

import com.android.tools.lint.client.api.UElementHandler
import com.android.tools.lint.detector.api.Category
import com.android.tools.lint.detector.api.Detector
import com.android.tools.lint.detector.api.Implementation
import com.android.tools.lint.detector.api.Issue
import com.android.tools.lint.detector.api.JavaContext
import com.android.tools.lint.detector.api.Scope
import com.android.tools.lint.detector.api.Severity
import com.android.tools.lint.detector.api.SourceCodeScanner
import com.intellij.psi.PsiMethod
import org.jetbrains.uast.UCallExpression
import org.jetbrains.uast.UElement
import org.jetbrains.uast.UExpression
import org.jetbrains.uast.UIfExpression
import org.jetbrains.uast.ULiteralExpression
import org.jetbrains.uast.UParenthesizedExpression
import org.jetbrains.uast.UPolyadicExpression
import org.jetbrains.uast.UastBinaryOperator

/**
 * Flags hardcoded user-facing text in Compose UI and notifications.
 *
 * A string literal (or string template / concatenation) that contains at least one
 * letter is reported when it is passed
 *  - to a text-like parameter ([TEXT_PARAMETERS]) of any `@Composable` function,
 *  - as the first argument of `androidx.compose.material3.Text`,
 *  - to `Notification(Compat).Builder.setContentTitle/setContentText/setTicker`,
 *    the `name` of `NotificationChannel(...)` or the text of `Toast.makeText(...)`.
 *
 * Literals without letters ("#", "99+", "/%1$d", emoji) are ignored; format specifiers
 * do not count as letters. Compose animation APIs (`androidx.compose.animation.*`) are
 * skipped because their `label` is a tooling label, not user-facing text.
 */
class ComposeHardcodedTextDetector : Detector(), SourceCodeScanner {

    override fun getApplicableUastTypes(): List<Class<out UElement>> =
        listOf(UCallExpression::class.java)

    override fun createUastHandler(context: JavaContext): UElementHandler =
        object : UElementHandler() {
            override fun visitCallExpression(node: UCallExpression) {
                checkCall(context, node)
            }
        }

    private fun checkCall(context: JavaContext, node: UCallExpression) {
        val method = node.resolve() ?: return
        val ownerClass = method.containingClass?.qualifiedName
        // Compose animation APIs (animate*AsState, updateTransition, Crossfade, ...) take a
        // `label` that only shows up in tooling, never on screen.
        val isAnimationApi = ownerClass?.startsWith(ANIMATION_PACKAGE) == true
        val isComposable = !isAnimationApi &&
            method.annotations.any { it.qualifiedName == COMPOSABLE }
        val isMaterial3Text = method.name == "Text" &&
            ownerClass?.startsWith("androidx.compose.material3.") == true
        val platformTextIndex = platformTextArgumentIndex(method, ownerClass)

        if (!isComposable && !isMaterial3Text && platformTextIndex == null) return

        val mapping = context.evaluator.computeArgumentMapping(node, method)
        val parameters = method.parameterList.parameters
        for ((argument, parameter) in mapping) {
            val index = parameters.indexOf(parameter)
            val textLike = (isComposable && parameter.name in TEXT_PARAMETERS) ||
                (isMaterial3Text && index == 0) ||
                (platformTextIndex != null && index == platformTextIndex)
            if (!textLike || !containsHardcodedText(argument)) continue

            context.report(
                ISSUE,
                argument,
                context.getLocation(argument),
                "Hardcoded text passed to `${method.name}`; use a string resource " +
                    "(`stringResource` / `getString`)",
            )
        }
    }

    /** Index of the user-facing text parameter for platform/notification APIs, or null. */
    private fun platformTextArgumentIndex(method: PsiMethod, ownerClass: String?): Int? =
        when {
            ownerClass in NOTIFICATION_BUILDERS && method.name in NOTIFICATION_TEXT_METHODS -> 0
            ownerClass == "android.app.NotificationChannel" && method.isConstructor -> 1
            ownerClass == "android.widget.Toast" && method.name == "makeText" -> 1
            else -> null
        }

    private fun containsHardcodedText(expression: UExpression?): Boolean =
        when (expression) {
            null -> false
            is UParenthesizedExpression -> containsHardcodedText(expression.expression)
            is UIfExpression -> containsHardcodedText(expression.thenExpression) ||
                containsHardcodedText(expression.elseExpression)
            else -> literalText(expression)?.let { hasLetters(it) } == true
        }

    /**
     * The literal text of a string literal, Kotlin string template or `+` concatenation, with
     * non-literal parts replaced by [PLACEHOLDER]; null if the expression has no string literal.
     * Literal parts are joined first because Kotlin UAST may split one template into several
     * literal operands (e.g. around an escaped `\$`).
     */
    private fun literalText(expression: UExpression?): String? =
        when (expression) {
            is UParenthesizedExpression -> literalText(expression.expression)
            is ULiteralExpression -> expression.value as? String
            is UPolyadicExpression -> {
                if (expression.operator != UastBinaryOperator.PLUS) {
                    null
                } else {
                    val parts = expression.operands.map { literalText(it) }
                    if (parts.all { it == null }) null else parts.joinToString("") { it ?: PLACEHOLDER }
                }
            }
            else -> null
        }

    private fun hasLetters(text: String): Boolean =
        LETTER.containsMatchIn(FORMAT_SPECIFIER.replace(text, ""))

    companion object {
        private const val COMPOSABLE = "androidx.compose.runtime.Composable"
        private const val ANIMATION_PACKAGE = "androidx.compose.animation."
        private const val PLACEHOLDER = "\u0000"
        private val LETTER = Regex("[A-Za-z]")

        /** Format specifiers like `%d` or `%1$s` are not words. */
        private val FORMAT_SPECIFIER = Regex("%[0-9]*\\$?[a-zA-Z]")

        private val TEXT_PARAMETERS = setOf(
            "text",
            "contentDescription",
            "label",
            "title",
            "message",
            "placeholder",
            "supportingText",
            "onClickLabel",
        )

        private val NOTIFICATION_BUILDERS = setOf(
            "androidx.core.app.NotificationCompat.Builder",
            "android.app.Notification.Builder",
        )

        private val NOTIFICATION_TEXT_METHODS = setOf("setContentTitle", "setContentText", "setTicker")

        @JvmField
        val ISSUE: Issue = Issue.create(
            id = "ComposeHardcodedText",
            briefDescription = "Hardcoded user-facing text",
            explanation = "User-facing text must come from strings.xml (stringResource / getString), " +
                "see mobile/android/AGENTS.md.",
            category = Category.I18N,
            priority = 6,
            severity = Severity.ERROR,
            implementation = Implementation(
                ComposeHardcodedTextDetector::class.java,
                Scope.JAVA_FILE_SCOPE,
            ),
        )
    }
}
