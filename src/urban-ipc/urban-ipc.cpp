#include <QCoreApplication>
#include <QRemoteObjectDynamicReplica>
#include <QRemoteObjectNode>
#include <QStringList>
#include <QTextStream>
#include <QTimer>
#include <QTcpSocket>
#include <cstdlib>
#include <vector>
#include <winsock2.h>
#include <ws2tcpip.h>
#include <windows.h>
#include <iphlpapi.h>

static bool urbanTunnelUp()
{
    ULONG size = 16 * 1024;
    std::vector<unsigned char> storage(size);
    auto *addresses = reinterpret_cast<IP_ADAPTER_ADDRESSES *>(storage.data());
    ULONG result = GetAdaptersAddresses(
        AF_UNSPEC,
        GAA_FLAG_INCLUDE_ALL_INTERFACES,
        nullptr,
        addresses,
        &size);
    if (result == ERROR_BUFFER_OVERFLOW) {
        storage.resize(size);
        addresses = reinterpret_cast<IP_ADAPTER_ADDRESSES *>(storage.data());
        result = GetAdaptersAddresses(
            AF_UNSPEC,
            GAA_FLAG_INCLUDE_ALL_INTERFACES,
            nullptr,
            addresses,
            &size);
    }
    if (result != NO_ERROR)
        return false;

    for (auto *current = addresses; current; current = current->Next) {
        const QString name = QString::fromWCharArray(current->FriendlyName);
        if (name.compare(QStringLiteral("UrbanVPN"), Qt::CaseInsensitive) == 0)
            return current->OperStatus == IfOperStatusUp;
    }
    return false;
}

int main(int argc, char *argv[])
{
    QCoreApplication app(argc, argv);
    QTextStream out(stdout);
    QTextStream err(stderr);
    const auto finish = [&out, &err](int code) {
        out.flush();
        err.flush();
        std::_Exit(code);
    };
    const QStringList arguments = app.arguments();
    const QString action = arguments.value(1, QStringLiteral("status")).toLower();
    const QString location = arguments.value(2, QStringLiteral("us")).toLower();

    if (action != QStringLiteral("status")
        && action != QStringLiteral("connect")
        && action != QStringLiteral("disconnect")) {
        err << "uso: urban-ipc.exe [status|connect <pais>|disconnect]" << Qt::endl;
        return 1;
    }

    if (action == QStringLiteral("status")) {
        const bool up = urbanTunnelUp();
        out << "tunnel=" << (up ? "up" : "down") << Qt::endl;
        finish(up ? 0 : 5);
    }

    const bool targetUp = action == QStringLiteral("connect");
    if (targetUp && urbanTunnelUp()) {
        out << "tunnel=up already=true" << Qt::endl;
        finish(0);
    }

    QRemoteObjectNode node;
    QTcpSocket socket;
    socket.connectToHost(QStringLiteral("127.0.0.1"), 19206);
    if (!socket.waitForConnected(3000)) {
        err << "conexão com o serviço UrbanVPN falhou: "
            << socket.errorString() << Qt::endl;
        return 2;
    }
    node.addClientSideConnection(&socket);

    QRemoteObjectDynamicReplica *replica = node.acquireDynamic(QStringLiteral("ServiceApi"));
    if (!replica) {
        err << "ServiceApi não pôde ser criada" << Qt::endl;
        return 3;
    }
    bool initialized = false;

    QObject::connect(replica, &QRemoteObjectDynamicReplica::initialized, &app, [&]() {
        initialized = true;
        const QString requested = action == QStringLiteral("disconnect")
            ? QString()
            : location;
        const bool streamingInvoked = QMetaObject::invokeMethod(
            replica,
            "pushSaIsStreamingMode",
            Qt::DirectConnection,
            Q_ARG(bool, false));
        const bool locationInvoked = QMetaObject::invokeMethod(
            replica,
            "pushSaRequestedLocation",
            Qt::DirectConnection,
            Q_ARG(QString, requested));
        const bool invoked = streamingInvoked && locationInvoked;
        out << "command=" << action
            << " location=" << requested
            << " streaming=" << (streamingInvoked ? "true" : "false")
            << " location_invoked=" << (locationInvoked ? "true" : "false") << Qt::endl;
        if (!invoked) {
            finish(4);
            return;
        }
        // O serviço envia logo depois um VCT::VpnStatus privado. Um cliente
        // externo não possui esse metatipo e não deve permanecer conectado
        // para recebê-lo. A confirmação do adaptador é feita pelo chamador,
        // em um novo processo `status`, sem depender dos tipos do Urban.
        socket.flush();
        socket.waitForBytesWritten(1000);
        finish(0);
    });

    QTimer::singleShot(7000, &app, [&]() {
        if (initialized)
            return;
        err << "ServiceApi não inicializou; estado=" << int(replica->state())
            << " erro=" << node.lastError() << Qt::endl;
        finish(3);
    });
    return app.exec();
}
