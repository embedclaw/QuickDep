#import <Foundation/Foundation.h>
#import "Logger.h"

@protocol ResultLogging
- (void)logResult:(NSInteger)value;
@end

@interface Calculator : NSObject <ResultLogging>
@property(nonatomic) NSInteger result;
- (NSInteger)add:(NSInteger)a to:(NSInteger)b;
- (void)reset;
+ (Calculator *)sharedCalculator;
@end

@implementation Calculator

- (NSInteger)add:(NSInteger)a to:(NSInteger)b {
    NSInteger sum = a + b;
    self.result = sum;
    [self logResult:sum];
    return sum;
}

- (void)reset {
    self.result = 0;
    NSLog(@"Calculator reset");
}

- (void)logResult:(NSInteger)value {
    NSLog(@"Result: %ld", (long)value);
}

+ (Calculator *)sharedCalculator {
    static Calculator *instance = nil;
    if (instance == nil) {
        instance = [[Calculator alloc] init];
    }
    return instance;
}

@end

int main(int argc, const char * argv[]) {
    @autoreleasepool {
        Calculator *calc = [Calculator sharedCalculator];
        NSInteger r = [calc add:3 to:4];
        [calc reset];
        NSLog(@"Final: %ld", (long)r);
    }
    return 0;
}
