import java.nio.charset.StandardCharsets;
import java.util.*;
import com.fasterxml.jackson.databind.ObjectMapper;
public class KafkaPartitionOracle {
    public static void main(String[] args) throws Exception {
        var rows=new ArrayList<Map<String,Object>>();
        for(var key: List.of("","key","native-key","中文键","kafka-🧅","same-key","1","ab","abc","abcd","abcde")) {
            rows.add(Map.of("key",key,"hash",org.apache.kafka.common.utils.Utils.murmur2(key.getBytes(StandardCharsets.UTF_8))));
        }
        System.out.println(new ObjectMapper().writeValueAsString(rows));
    }
}
