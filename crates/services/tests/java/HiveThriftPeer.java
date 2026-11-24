// Independent Apache-generated HS2 peer. Loopback fixtures only; no user's Hive/JDBC configuration.
import java.net.*;
import java.nio.ByteBuffer;
import java.util.*;
import java.lang.reflect.Proxy;
import java.security.*;
import javax.security.auth.callback.CallbackHandler;
import javax.security.auth.Subject;
import javax.security.auth.login.LoginContext;
import javax.security.sasl.*;
import org.apache.hive.service.rpc.thrift.*;
import org.apache.thrift.protocol.TBinaryProtocol;
import org.apache.thrift.server.*;
import org.apache.thrift.transport.*;
import org.ietf.jgss.*;
import com.sun.net.httpserver.HttpServer;

public final class HiveThriftPeer {
    private static boolean active, fetched, closed, failed;
    private static int executions;
    private static boolean kerberos;
    private static TServer server;
    private static TStatus ok() { return new TStatus(TStatusCode.SUCCESS_STATUS); }
    private static THandleIdentifier id() { return new THandleIdentifier(ByteBuffer.wrap(new byte[16]), ByteBuffer.wrap(new byte[16])); }
    private static void require(boolean value) { if (!value) { failed = true; throw new IllegalStateException("Fixture contract failed"); } }
    private static TColumnDesc column(String name, TTypeId type, int position) {
        var primitive = new TPrimitiveTypeEntry(type);
        var entry = new TTypeEntry(); entry.setPrimitiveEntry(primitive);
        return new TColumnDesc(name, new TTypeDesc(List.of(entry)), position);
    }
    private static Object call(String method, Object value) {
        switch (method) {
            case "OpenSession": {
                var request = (TOpenSessionReq)value;
                require(request.getClient_protocol() == TProtocolVersion.HIVE_CLI_SERVICE_PROTOCOL_V6);
                require("fixture-user".equals(request.getUsername()) && (kerberos?"":"fixture-only").equals(request.getPassword()));
                require("fixture_database".equals(request.getConfiguration().get("use:database")));
                var response = new TOpenSessionResp(ok(), TProtocolVersion.HIVE_CLI_SERVICE_PROTOCOL_V6);
                response.setSessionHandle(new TSessionHandle(id())); return response;
            }
            case "GetInfo": {
                var info = new TGetInfoValue(); info.setStringValue("4.1.0-generated-fixture");
                return new TGetInfoResp(ok(),info);
            }
            case "ExecuteStatement": {
                require(!active);
                var request = (TExecuteStatementReq)value;
                require("SELECT 42, '中文', unhex('00FF'), NULL".equals(request.getStatement()));
                require(request.isRunAsync());
                active=true;fetched=false;executions++;
                var response=new TExecuteStatementResp(ok());
                response.setOperationHandle(new TOperationHandle(id(),TOperationType.EXECUTE_STATEMENT,true));
                return response;
            }
            case "GetOperationStatus": {
                require(active);
                var response=new TGetOperationStatusResp(ok());response.setOperationState(TOperationState.FINISHED_STATE);
                response.setHasResultSet(true);return response;
            }
            case "GetResultSetMetadata": {
                require(active);
                var response=new TGetResultSetMetadataResp(ok());
                response.setSchema(new TTableSchema(List.of(column("id",TTypeId.BIGINT_TYPE,1),column("中文列",TTypeId.STRING_TYPE,2),column("payload",TTypeId.BINARY_TYPE,3),column("missing",TTypeId.STRING_TYPE,4))));return response;
            }
            case "FetchResults": {
                require(active);var request=(TFetchResultsReq)value;require(request.getMaxRows()==256);
                var columns=new ArrayList<TColumn>();
                var integers=new TColumn();integers.setI64Val(new TI64Column(fetched?List.of():List.of(Long.MAX_VALUE),ByteBuffer.wrap(new byte[0])));columns.add(integers);
                var strings=new TColumn();strings.setStringVal(new TStringColumn(fetched?List.of():List.of("中文"),ByteBuffer.wrap(new byte[0])));columns.add(strings);
                var binary=new TColumn();binary.setBinaryVal(new TBinaryColumn(fetched?List.of():List.of(ByteBuffer.wrap(new byte[]{0,(byte)255})),ByteBuffer.wrap(new byte[0])));columns.add(binary);
                var nulls=new TColumn();nulls.setStringVal(new TStringColumn(fetched?List.of():List.of(""),ByteBuffer.wrap(fetched?new byte[0]:new byte[]{1})));columns.add(nulls);
                var rows=new TRowSet(0,List.of());rows.setColumns(columns);fetched=true;
                var response=new TFetchResultsResp(ok());response.setHasMoreRows(false);response.setResults(rows);return response;
            }
            case "CloseOperation": require(active);active=false;return new TCloseOperationResp(ok());
            case "CloseSession": require(!active && executions==1);closed=true;if(server!=null)server.stop();return new TCloseSessionResp(ok());
            default: failed=true;throw new IllegalStateException("Unexpected RPC: "+method);
        }
    }
    public static final class PlainFactory implements SaslServerFactory {
        public SaslServer createSaslServer(String mechanism,String protocol,String name,Map<String,?> properties,CallbackHandler handler) throws SaslException {
            if (!"PLAIN".equals(mechanism)) return null;
            return new SaslServer() {
                private boolean complete;
                public String getMechanismName(){return "PLAIN";}
                public byte[] evaluateResponse(byte[] value) throws SaslException {
                    require(Arrays.equals(value,"\0fixture-user\0fixture-only".getBytes(java.nio.charset.StandardCharsets.UTF_8)));
                    complete=true;return new byte[0];
                }
                public boolean isComplete(){return complete;}
                public String getAuthorizationID(){return "fixture-user";}
                public byte[] unwrap(byte[] bytes,int start,int length){return Arrays.copyOfRange(bytes,start,start+length);}
                public byte[] wrap(byte[] bytes,int start,int length){return Arrays.copyOfRange(bytes,start,start+length);}
                public Object getNegotiatedProperty(String key){return Sasl.QOP.equals(key)?"auth":null;}
                public void dispose(){}
            };
        }
        public String[] getMechanismNames(Map<String,?> properties){return new String[]{"PLAIN"};}
    }
    public static void main(String[] args) throws Exception {
        if (args.length==1 && (args[0].startsWith("gss-") || args[0].startsWith("http-gss"))) {
            var login=new LoginContext("TomatoFixture");login.login();
            try {Subject.callAs(login.getSubject(),()->{serve(args[0]);return null;});}
            finally {login.logout();}
        }else{serve(args.length==1?args[0]:"plain");}
    }
    private static void serve(String mode) throws Exception {
        kerberos=mode.startsWith("gss-") || mode.startsWith("http-gss");
        var iface=(TCLIService.Iface)Proxy.newProxyInstance(HiveThriftPeer.class.getClassLoader(),new Class[]{TCLIService.Iface.class},(proxy,method,values)->call(method.getName(),values[0]));
        if (mode.startsWith("http-")) {serveHttp(mode,iface);return;}
        var socket=new ServerSocket(0,16,InetAddress.getByName("127.0.0.1"));
        var options=new TSimpleServer.Args(new TServerSocket(socket)).processor(new TCLIService.Processor<>(iface)).protocolFactory(new TBinaryProtocol.Factory(true,true));
        if (mode.equals("sasl")) {
            var provider=new Provider("TomatoFixturePlain","1.0","Loopback SASL PLAIN fixture"){};
            provider.put("SaslServerFactory.PLAIN",PlainFactory.class.getName());Security.addProvider(provider);
            var transports=new TSaslServerTransport.Factory();
            transports.addServerDefinition("PLAIN","hive","localhost",Map.of(Sasl.QOP,"auth"),callbacks->{});
            options.transportFactory(transports);
        }else if(mode.startsWith("gss-")) {
            String qop=mode.substring(4);require(Set.of("auth","auth-int","auth-conf").contains(qop));
            var transports=new TSaslServerTransport.Factory();
            transports.addServerDefinition("GSSAPI","hive","localhost",Map.of(Sasl.QOP,qop),callbacks->{
                for(var callback:callbacks){
                    if(callback instanceof AuthorizeCallback authorization){
                        boolean owned="fixture-user@TOMATO.TEST".equals(authorization.getAuthenticationID());
                        boolean same=authorization.getAuthenticationID().equals(authorization.getAuthorizationID());
                        authorization.setAuthorized(owned&&same);
                    }else{throw new javax.security.auth.callback.UnsupportedCallbackException(callback);}
                }
            });
            options.transportFactory(transports);
        }
        server=new TSimpleServer(options);
        System.out.println(socket.getLocalPort());System.out.flush();server.serve();
        if (failed || !closed) System.exit(2);
    }
    private static void serveHttp(String mode,TCLIService.Iface iface) throws Exception {
        Subject subject=Subject.current();
        var http=HttpServer.create(new InetSocketAddress("127.0.0.1",0),16);
        var processor=new TCLIService.Processor<>(iface);
        var seen=new HashSet<String>();
        var completion=new java.util.concurrent.CountDownLatch(1);
        int[] requests={0},tickets={0},expired={0};
        boolean cookieEnabled=!mode.endsWith("-nocookie");
        http.createContext("/fixture",exchange->{
            try {
                Subject.callAs(subject,()->{
                    try {
                        require(exchange.getRequestMethod().equals("POST"));
                        byte[] body=exchange.getRequestBody().readNBytes(8*1024*1024+1);require(body.length<=8*1024*1024);
                        requests[0]++;
                        String cookie=exchange.getRequestHeaders().getFirst("Cookie");
                        boolean valid=cookieEnabled && cookie!=null && cookie.contains("hive.server2.auth=fixture-session");
                        if(valid && mode.endsWith("-expiry") && expired[0]++==0){
                            exchange.getResponseHeaders().add("WWW-Authenticate","Negotiate");
                            exchange.sendResponseHeaders(401,-1);exchange.close();return null;
                        }
                        if(!valid){
                            String header=exchange.getRequestHeaders().getFirst("Authorization");
                            tickets[0]++;
                            if(kerberos){
                                require(header!=null && header.startsWith("Negotiate "));
                                String encoded=header.substring(10);require(seen.add(encoded));
                                var context=GSSManager.getInstance().createContext((GSSCredential)null);
                                try {
                                    byte[] token=Base64.getDecoder().decode(encoded);context.acceptSecContext(token,0,token.length);
                                    require(context.isEstablished());require(context.getSrcName().toString().equals("fixture-user@TOMATO.TEST"));
                                }finally{context.dispose();}
                            }else{
                                require(("Basic "+Base64.getEncoder().encodeToString("fixture-user:fixture-only".getBytes(java.nio.charset.StandardCharsets.UTF_8))).equals(header));
                            }
                        }
                        var input=new TMemoryInputTransport(body);var output=new TMemoryBuffer(1024);
                        processor.process(new TBinaryProtocol(input,true,true),new TBinaryProtocol(output,true,true));
                        if(cookieEnabled)exchange.getResponseHeaders().add("Set-Cookie","hive.server2.auth=fixture-session; HttpOnly; Path=/fixture");
                        exchange.getResponseHeaders().add("Content-Type","application/x-thrift");
                        exchange.sendResponseHeaders(200,output.length());
                        try(var stream=exchange.getResponseBody()){stream.write(output.getArray(),0,output.length());}
                        if(closed)completion.countDown();
                        return null;
                    }catch(Exception error){failed=true;completion.countDown();throw new RuntimeException(error);}
                });
            }catch(Exception error){failed=true;exchange.close();completion.countDown();}
        });
        http.start();System.out.println(http.getAddress().getPort());System.out.flush();
        try {require(completion.await(25,java.util.concurrent.TimeUnit.SECONDS));}
        finally {http.stop(0);}
        require(!failed && closed);
        require(mode.endsWith("-nocookie")?tickets[0]==requests[0]:tickets[0]==(mode.endsWith("-expiry")?2:1));
    }
}
