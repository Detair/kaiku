package io.wolftown.kaiku.data.keydevice

import io.wolftown.kaiku.approval.GeneratedKey
import io.wolftown.kaiku.approval.SoftwareApprovalKeyStore
import io.wolftown.kaiku.approval.UvClass
import io.wolftown.kaiku.approval.UvDecision
import io.wolftown.kaiku.approval.UvPolicy
import io.wolftown.kaiku.approval.keyId
import io.wolftown.kaiku.data.local.AuthState
import io.wolftown.kaiku.testutil.InMemoryKeyDeviceStore
import kotlinx.coroutines.test.TestScope
import kotlinx.coroutines.test.runTest
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test
import java.security.KeyFactory
import java.security.Signature
import java.security.spec.X509EncodedKeySpec
import kotlin.random.Random

/**
 * What two parts of the app can do to the one record and the one key, as the storage boundary
 * offers it. The test below drives it; nothing else reaches the store or the alias.
 */
internal interface Vault {
    fun stored(): StoredKeyDevice?
    fun aliasKeyId(): String?
    fun generate(): GeneratedKey?
    fun create(record: StoredKeyDevice): Boolean
    fun replace(expected: StoredKeyDevice, next: StoredKeyDevice): Boolean
    fun forget(expected: StoredKeyDevice): Boolean
    fun deleteUnclaimed(keyId: String): Boolean
    /** A signature over [message] made for the holder of [key], or null when none is made. */
    fun sign(key: GeneratedKey, message: ByteArray): ByteArray?
}

/** The storage boundary of the app: [KeyDeviceRepository] and the Keystore wrapper behind it. */
internal class RepositoryVault(private val repo: KeyDeviceRepository, private val keyStore: SoftwareApprovalKeyStore) : Vault {
    private val use = UvPolicy.choose(30, true, true) as UvDecision.Use
    override fun stored() = repo.record.value
    override fun aliasKeyId() = keyStore.keyId()
    override fun generate() = (repo.generateKey(ByteArray(32), use) as? NewKey.Made)?.key
    override fun create(record: StoredKeyDevice) = repo.create(record).done
    override fun replace(expected: StoredKeyDevice, next: StoredKeyDevice) = repo.replace(expected, next).done
    override fun forget(expected: StoredKeyDevice) = repo.forget(expected, Forget.SERVER_ANSWER).done
    override fun deleteUnclaimed(keyId: String) = repo.deleteUnclaimedKey(keyId)
    override fun sign(key: GeneratedKey, message: ByteArray): ByteArray? = try {
        keyStore.signature(key.keyId).run {
            update(message)
            sign()
        }
    } catch (_: Exception) {
        null
    }
}

/**
 * Property: whatever two actors do in whatever order, each with expectations that may be stale,
 *  (1) a key the server has accepted is never lost: its record stays stored and the alias keeps
 *      holding it until the server itself removes it;
 *  (2) no actor ever gets a signature from a key it did not make itself.
 *
 * Each actor follows the app's own order (a key, then its record, then the signed claim) but is
 * interleaved freely with the other, and both also replay what they saw earlier: writes over and
 * forgetting of records that are no longer the stored one, as a late answer or an older screen would.
 */
class KeyVaultInterleavingTest {

    private class Actor(val name: String) {
        var key: GeneratedKey? = null
        var record: StoredKeyDevice? = null
        /** Records this actor wrote or saw earlier. */
        val memory = mutableListOf<StoredKeyDevice>()
        var setups = 0
    }

    /** The server as far as the property needs it: which keys it has stored, and which it removed. */
    private class Server {
        val accepted = mutableSetOf<String>()
        val removed = mutableSetOf<String>()
    }

    private fun verifies(spki: ByteArray, message: ByteArray, sig: ByteArray): Boolean {
        val pub = KeyFactory.getInstance("EC").generatePublic(X509EncodedKeySpec(spki))
        return Signature.getInstance("SHA256withECDSA").run {
            initVerify(pub)
            update(message)
            verify(sig)
        }
    }

    private fun device(actor: Actor) = LocalKeyDevice(
        keyDeviceId = null, userId = "user", origin = "https://chat.example.com", uvClass = UvClass.PER_USE,
        enrollmentId = "enrolment-${actor.name}-${actor.setups}", claimSecret = null,
    )

    private fun run(vault: Vault, seed: Int, steps: Int) {
        val rnd = Random(seed)
        val server = Server()
        val actors = listOf(Actor("a"), Actor("b"))
        val trace = mutableListOf<String>()
        fun check(step: String) {
            trace += step
            for (k in server.accepted) {
                val why = "seed $seed: the server holds key $k but the phone lost it after ${trace.takeLast(12)}"
                assertEquals(why, k, vault.stored()?.keyId)
                assertEquals(why, k, vault.aliasKeyId())
            }
        }
        repeat(steps) {
            val me = actors[rnd.nextInt(2)]
            when (rnd.nextInt(10)) {
                0 -> if (me.key == null && me.record == null) {
                    me.key = vault.generate()
                    if (me.key != null) me.setups++
                    check("${me.name} makes a key")
                }
                1 -> me.key?.let { k ->
                    if (me.record == null) {
                        val rec = StoredKeyDevice(device(me), KeyDevicePhase.CLAIMING, keyId = k.keyId, attemptId = "${me.name}-${me.setups}")
                        if (vault.create(rec)) {
                            me.record = rec
                            me.memory += rec
                        }
                        check("${me.name} records its claim")
                    }
                }
                2 -> me.key?.let { k ->
                    val message = "claim of ${me.name} ${me.setups}".toByteArray()
                    val sig = vault.sign(k, message)
                    if (sig != null) {
                        assertTrue("seed $seed: ${me.name} got a signature of a key it did not make", verifies(k.spki, message, sig))
                        // The claim leaves only with its record stored; the server then holds the key.
                        if (me.record != null && k.keyId !in server.removed) server.accepted += k.keyId
                    }
                    check("${me.name} signs and claims")
                }
                3 -> me.record?.let { rec ->
                    if (rec.keyId in server.accepted && rec.phase != KeyDevicePhase.ACTIVE) {
                        val next = rec.copy(phase = if (rec.phase == KeyDevicePhase.CLAIMING) KeyDevicePhase.CLAIMED else KeyDevicePhase.ACTIVE)
                        if (vault.replace(rec, next)) {
                            me.record = next
                            me.memory += next
                        }
                        check("${me.name} records the server's answer")
                    }
                }
                // A refusal of this actor's own claim: the server holds nothing for its key.
                4 -> me.record?.let { rec ->
                    if (rec.keyId !in server.accepted) {
                        if (vault.forget(rec)) {
                            me.record = null
                            me.key = null
                        }
                        check("${me.name} is refused and forgets")
                    }
                }
                // A late answer about a record seen earlier: the server says that one is over.
                5 -> me.memory.randomOrNull(rnd)?.let { seen ->
                    if (seen.keyId !in server.accepted) {
                        if (vault.forget(seen)) actors.forEach { a ->
                            if (a.record?.sameAs(seen) == true) {
                                a.record = null
                                a.key = null
                            }
                        }
                        check("${me.name} forgets a record it saw earlier")
                    }
                }
                // An older screen writes what it believed to be the state.
                6 -> me.memory.randomOrNull(rnd)?.let { seen ->
                    val wrote = if (rnd.nextBoolean()) vault.create(seen) else vault.replace(seen, seen.copy(phase = KeyDevicePhase.CLAIMED))
                    if (wrote) actors.forEach { a -> if (a.record?.sameAs(seen) == true) a.record = vault.stored() }
                    check("${me.name} writes from an old view")
                }
                7 -> me.key?.let { k ->
                    if (me.record == null) {
                        vault.deleteUnclaimed(k.keyId)
                        me.key = null
                        check("${me.name} abandons its unclaimed key")
                    }
                }
                8 -> vault.stored()?.let {
                    me.memory += it
                    check("${me.name} looks at the stored record")
                }
                // The server removes a key it held (an admin did); forgetting it is right from then on.
                else -> server.accepted.randomOrNull(rnd)?.let { k ->
                    server.accepted -= k
                    server.removed += k
                    check("the server removes a key")
                }
            }
        }
    }

    private fun TestScope.vault(): Vault {
        val keyStore = SoftwareApprovalKeyStore()
        val repo = KeyDeviceRepository(InMemoryKeyDeviceStore(), keyStore, AuthState(backgroundScope), backgroundScope)
        return RepositoryVault(repo, keyStore)
    }

    @Test fun `an accepted key is never lost and nobody signs with another's key, in any interleaving`() = runTest {
        for (seed in 1..400) run(vault(), seed, steps = 80)
    }

    /** One fixed order: an older setup goes on from what it believed after a newer one claimed. */
    @Test fun `an older setup can neither write over nor forget a newer setup's claim`() = runTest {
        val vault = vault()
        val older = vault.generate()!!
        // A newer setup replaces the unclaimed key, records its claim, and the server stores it.
        vault.deleteUnclaimed(older.keyId)
        val newer = vault.generate()!!
        val claim = StoredKeyDevice(LocalKeyDevice(null, "user", "https://chat.example.com", UvClass.PER_USE, "e2", null), KeyDevicePhase.CLAIMING, keyId = newer.keyId, attemptId = "newer")
        assertTrue(vault.create(claim))
        // The older setup goes on from what it believed: it cannot sign, record or forget.
        assertEquals(null, vault.sign(older, "claim".toByteArray()))
        val own = claim.copy(device = claim.device.copy(enrollmentId = "e1"), keyId = older.keyId, attemptId = "older")
        assertEquals(false, vault.create(own))
        assertEquals(false, vault.forget(own))
        assertEquals(false, vault.deleteUnclaimed(older.keyId))
        assertEquals(claim, vault.stored())
        assertEquals(newer.keyId, vault.aliasKeyId())
    }
}
