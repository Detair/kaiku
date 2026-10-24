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
 * Literals without letters ("#", "99+", "/%1$d", emoji) are ignored.
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
        val isComposable = method.annotations.any { it.qualifiedName == COMPOSABLE }
        val ownerClass = method.containingClass?.qualifiedName
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
            is ULiteralExpression -> (expression.value as? String)?.let { LETTER.containsMatchIn(it) } == true
            // Kotlin string templates and `+` concatenations are polyadic PLUS expressions.
            is UPolyadicExpression -> expression.operator == UastBinaryOperator.PLUS &&
                expression.operands.any { containsHardcodedText(it) }
            is UIfExpression -> containsHardcodedText(expression.thenExpression) ||
                containsHardcodedText(expression.elseExpression)
            else -> false
        }

    companion object {
        private const val COMPOSABLE = "androidx.compose.runtime.Composable"
        private val LETTER = Regex("[A-Za-z]")

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
