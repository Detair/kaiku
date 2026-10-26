package io.wolftown.kaiku.lint

import com.android.tools.lint.client.api.IssueRegistry
import com.android.tools.lint.client.api.Vendor
import com.android.tools.lint.detector.api.CURRENT_API
import com.android.tools.lint.detector.api.Issue

/** Registers Kaiku's custom lint checks (discovered via META-INF/services). */
class KaikuIssueRegistry : IssueRegistry() {
    override val issues: List<Issue> = listOf(ComposeHardcodedTextDetector.ISSUE)

    override val api: Int = CURRENT_API

    override val vendor: Vendor = Vendor(vendorName = "Kaiku")
}
