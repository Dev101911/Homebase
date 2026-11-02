import Foundation
import Testing
@testable import Homebase

@Suite struct ModelTests {
    @Test func slugsMatchTheCore() {
        #expect(Slug.make("My App") == "my-app")
        #expect(Slug.make("  NotesApp!! ") == "notesapp")
        #expect(Slug.make("my_game") == "my-game")
        #expect(Slug.make("???") == "server")
        #expect(Slug.isValid("my-app2"))
        #expect(!Slug.isValid("My App"))
        #expect(!Slug.isValid("-x"))
    }

    @Test func decodesAgentStatus() throws {
        let json = """
        {"id":"my-app","name":"My App","folder":"~/x","state":"needs-attention","pid":12,
         "startedAt":1793620000,"restarts":0,"lastExit":null,
         "attention":"waiting for the folder-trust question","command":null,
         "lastLines":["a"],"logFile":"/tmp/my-app.log"}
        """
        let status = try JSONDecoder().decode(ServerStatus.self, from: Data(json.utf8))
        #expect(status.state == .needsAttention)
        #expect(status.needsTrust)
        #expect(status.state.isLive)
    }

    @Test func requestsLeaveOutNils() throws {
        let data = try JSONEncoder().encode(AgentRequest.start(["a"]))
        let object = try JSONSerialization.jsonObject(with: data) as? [String: Any]
        #expect(object?["cmd"] as? String == "start")
        #expect(object?["ids"] as? [String] == ["a"])
        #expect(object?["server"] == nil)
    }

    @Test func serverRoundTripsWithNulls() throws {
        let json = """
        {"id":"a","name":"A","folder":"/tmp","spawn":"auto","createSessionInDir":null,"capacity":null,
         "permissionMode":null,"chrome":null,"namePrefix":null,"extraArgs":[],"env":{},
         "autoStart":true,"restart":"on-failure"}
        """
        let server = try JSONDecoder().decode(Server.self, from: Data(json.utf8))
        #expect(server.createSessionInDir == nil)
        let again = try JSONDecoder().decode(Server.self, from: JSONEncoder().encode(server))
        #expect(again == server)
    }

    @Test func uptime() {
        #expect(TimeInterval(45).uptimeText == "45s")
        #expect(TimeInterval(3 * 3600 + 12 * 60).uptimeText == "3h 12m")
        #expect(TimeInterval(2 * 86_400 + 4 * 3600).uptimeText == "2d 4h")
    }
}
