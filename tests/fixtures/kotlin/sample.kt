package acme.sample

import kotlin.math.max
import acme.shared.Helper as SharedHelper
import acme.shared.*

interface Greeter {
    fun greet(name: String): String
}

open class BaseService

class UserService(private val helper: SharedHelper) : BaseService(), Greeter {
    val name: String = ""

    override fun greet(name: String): String {
        val normalized = format(name)
        println(max(normalized.length, 1))
        return helper.decorate(normalized)
    }

    private fun format(name: String): String = name.trim()
}

object ServiceFactory {
    fun create(): UserService {
        return UserService(SharedHelper())
    }
}
